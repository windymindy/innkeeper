//! Game packet handling logic.

use std::collections::{HashMap, HashSet, VecDeque};

use bytes::Bytes;
use sha1::{Digest, Sha1};
use tracing::{debug, error, info, warn};

use crate::common::messages::GuildEventInfo;
use crate::common::types::{ChatMessage, GuildEvent, GuildInfo, GuildMember, Player};

/// Result of processing a chat message.
#[derive(Debug, Clone)]
pub enum ChatProcessingResult {
    Chat(ChatMessage),
    GuildEvent(GuildEventInfo),
}
use crate::protocol::game::chat::{
    get_language_for_race, ChannelNotify, ChatPlayerNotFound, JoinChannelWotLK, MessageChat,
    NameQuery, NameQueryResponse, SendChatMessage,
};
use crate::protocol::game::guild::{
    GuildEventPacket, GuildQuery, GuildQueryResponse, GuildRoster, GuildRosterRequest,
};
use crate::protocol::game::packets::{
    AuthChallenge, AuthResponse, AuthSession, CharEnum, CharEnumRequest, CharacterInfo, GameObjUse,
    InitWorldStates, KeepAlive, LoginVerifyWorld, Ping, PlayerLogin, Pong, TimeSyncReq,
    TimeSyncResp,
};
use crate::protocol::packets::PacketDecode;
use anyhow::{anyhow, Result};
use bytes::Buf;

/// Maximum number of distinct GUIDs with pending name resolution.
/// When exceeded, the oldest entry is evicted (messages silently dropped).
const MAX_PENDING_GUIDS: usize = 256;

/// Game protocol handler state.
pub struct GameHandler {
    account: String,
    session_key: [u8; 40],
    realm_id: u32,
    pub character_name: String,
    pub player: Option<Player>,
    pub self_guid: Option<u64>,
    pub guild_id: u32,
    pub language_id: u32,
    pub in_world: bool,

    /// Guild information (name, ranks)
    pub guild_info: Option<GuildQueryResponse>,
    /// Guild roster (members)
    pub guild_roster: HashMap<u64, GuildMember>,
    /// Guild MOTD
    pub guild_motd: Option<String>,
    /// Last time guild roster was requested (for periodic updates)
    pub last_roster_request: Option<std::time::Instant>,

    /// Cache of player names by GUID
    pub player_names: HashMap<u64, Player>,
    /// Pending chat messages waiting for name resolution
    pub pending_messages: HashMap<u64, Vec<ChatMessage>>,
    /// GUIDs that already have an in-flight CMSG_NAME_QUERY (avoids redundant queries)
    pub pending_name_queries: HashSet<u64>,
    /// Insertion-order tracking for pending_messages (front = oldest)
    pending_message_order: VecDeque<u64>,

    /// Sit quirk state
    pub tried_to_sit: bool,
    /// Last known world position (x, y, z)
    pub world_position: Option<(f32, f32, f32)>,
    /// Timestamp when the handler was created (for SMSG_TIME_SYNC_REQ uptime calculation)
    connect_time: std::time::Instant,
}

impl GameHandler {
    pub fn new(account: &str, session_key: [u8; 40], realm_id: u32, character_name: &str) -> Self {
        Self {
            account: account.to_string(),
            session_key,
            realm_id,
            character_name: character_name.to_string(),
            player: None,
            self_guid: None,
            guild_id: 0,
            language_id: 7, // Default to Common
            in_world: false,
            guild_info: None,
            guild_roster: HashMap::new(),
            guild_motd: None,
            last_roster_request: None,
            player_names: HashMap::new(),
            pending_messages: HashMap::new(),
            pending_name_queries: HashSet::new(),
            pending_message_order: VecDeque::new(),
            tried_to_sit: false,
            world_position: None,
            connect_time: std::time::Instant::now(),
        }
    }

    /// Handle SMSG_AUTH_CHALLENGE and build CMSG_AUTH_SESSION.
    pub fn handle_auth_challenge(&self, packet: AuthChallenge) -> Result<AuthSession> {
        let client_seed: u32 = rand::random();
        let server_seed = packet.server_seed;

        // Calculate SHA1 digest
        // SHA1(account + [0,0,0,0] + clientSeed + serverSeed + sessionKey)
        let mut hasher = Sha1::new();
        hasher.update(self.account.as_bytes());
        hasher.update(&[0, 0, 0, 0]);
        hasher.update(&client_seed.to_be_bytes());
        hasher.update(&server_seed.to_be_bytes());
        hasher.update(&self.session_key);
        let digest: [u8; 20] = hasher.finalize().into();

        debug!("Calculated auth digest for account {}", self.account);

        Ok(AuthSession {
            build: 12340, // WotLK 3.3.5a
            login_server_id: 0,
            account: self.account.clone(),
            login_server_type: 0,
            client_seed,
            region_id: 0,
            battlegroup_id: 0,
            realm_id: self.realm_id,
            dos_response: 3,
            digest,
        })
    }

    /// Handle SMSG_AUTH_RESPONSE.
    pub fn handle_auth_response(&self, packet: AuthResponse) -> Result<bool> {
        match packet {
            AuthResponse::Success { .. } => {
                debug!("Game auth successful!");
                Ok(true)
            }
            AuthResponse::Failure(code) => {
                error!("Game auth failed with code: 0x{:02X}", code);
                Ok(false)
            }
        }
    }

    /// Build CMSG_CHAR_ENUM request.
    pub fn build_char_enum_request(&self) -> CharEnumRequest {
        CharEnumRequest
    }

    /// Handle SMSG_CHAR_ENUM.
    pub fn handle_char_enum(
        &mut self,
        packet: CharEnum,
        character_name: &str,
    ) -> Option<CharacterInfo> {
        info!(
            "Received character list with {} characters",
            packet.characters.len()
        );

        for char_info in packet.characters {
            debug!(
                "Character: {} (GUID: {}, Level: {})",
                char_info.name, char_info.guid, char_info.level
            );
            if char_info.name.eq_ignore_ascii_case(character_name) {
                info!("Found character '{}'", char_info.name);
                self.self_guid = Some(char_info.guid);
                self.guild_id = char_info.guild_id;
                self.language_id = get_language_for_race(char_info.race);

                // Populate self.player
                self.player = Some(crate::common::types::Player {
                    guid: char_info.guid,
                    name: char_info.name.clone(),
                    level: char_info.level,
                    class: crate::common::resources::Class::from_id(char_info.class),
                    race: crate::common::resources::Race::from_id(char_info.race),
                    zone_id: char_info.zone_id,
                });

                return Some(char_info);
            }
        }

        None
    }

    /// Handle SMSG_LOGIN_VERIFY_WORLD.
    pub fn handle_login_verify_world(&mut self, packet: LoginVerifyWorld) -> Result<()> {
        info!(
            "World login verified! Map: {}, X: {}, Y: {}, Z: {}",
            packet.map_id, packet.x, packet.y, packet.z
        );
        self.in_world = true;
        Ok(())
    }

    /// Handle SMSG_PONG (optional, usually just logged).
    pub fn handle_pong(&self, packet: Pong) {
        debug!("Received PONG with sequence: {}", packet.sequence);
    }

    /// Handle SMSG_TIME_SYNC_REQ and build CMSG_TIME_SYNC_RESP.
    ///
    /// The server periodically sends this to synchronize time. We respond
    /// with the sync counter from the request and our uptime in milliseconds,
    /// matching the original Scala implementation.
    pub fn handle_time_sync_req(&self, packet: TimeSyncReq) -> TimeSyncResp {
        let uptime = self.connect_time.elapsed().as_millis() as u32;
        debug!(
            "Time sync request: counter={}, responding with uptime={}ms",
            packet.counter, uptime
        );
        TimeSyncResp {
            counter: packet.counter,
            client_ticks: uptime,
        }
    }

    /// Build CMSG_PLAYER_LOGIN.
    pub fn build_player_login(&self, guid: u64) -> PlayerLogin {
        PlayerLogin { guid }
    }

    /// Build CMSG_PING.
    pub fn build_ping(&self, sequence: u32) -> Ping {
        Ping {
            sequence,
            latency: 0,
        }
    }

    /// Build CMSG_KEEP_ALIVE (TBC/WotLK keepalive, empty packet).
    pub fn build_keep_alive(&self) -> KeepAlive {
        KeepAlive
    }

    /// Build CMSG_LOGOUT_REQUEST.
    pub fn build_logout_request(&self) -> crate::protocol::game::packets::LogoutRequest {
        crate::protocol::game::packets::LogoutRequest
    }

    // =========================================================================
    // Chat handling
    // =========================================================================

    /// Handle SMSG_MESSAGECHAT packet.
    pub fn handle_messagechat(
        &mut self,
        mut payload: Bytes,
    ) -> Result<Option<ChatProcessingResult>> {
        let msg = match MessageChat::decode(&mut payload) {
            Ok(msg) => msg,
            Err(e) if e.to_string().contains("skip") => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };

        // Handle Guild Achievements
        if msg.chat_type == crate::protocol::game::chat::chat_events::CHAT_MSG_GUILD_ACHIEVEMENT {
            // Look up player name from guild roster
            if let Some(member) = self.guild_roster.get(&msg.sender_guid) {
                let player_name = member.name.clone();

                return Ok(Some(ChatProcessingResult::GuildEvent(GuildEventInfo {
                    event_name: "achievement".to_string(),
                    player_name,
                    target_name: None,
                    rank_name: None,
                    achievement_id: msg.achievement_id,
                })));
            } else {
                warn!(
                    guid = %msg.sender_guid,
                    "Guild achievement from unknown player (not in roster yet)"
                );
                return Ok(None);
            }
        }

        match self.process_chat_message(msg)? {
            Some(chat_msg) => Ok(Some(ChatProcessingResult::Chat(chat_msg))),
            None => Ok(None),
        }
    }

    /// Handle SMSG_NAME_QUERY response.
    pub fn handle_name_query(&mut self, mut payload: Bytes) -> Result<Vec<ChatMessage>> {
        let response = NameQueryResponse::decode(&mut payload)?;

        // Add to cache
        let player = Player {
            guid: response.guid,
            name: response.name.clone(),
            level: 0,
            class: crate::common::resources::Class::from_id(response.class as u8),
            race: crate::common::resources::Race::from_id(response.race as u8),
            zone_id: 0,
        };
        self.player_names.insert(response.guid, player);
        self.pending_name_queries.remove(&response.guid);
        self.pending_message_order.retain(|&g| g != response.guid);

        // Process any pending messages for this GUID
        let mut resolved = Vec::new();
        if let Some(messages) = self.pending_messages.remove(&response.guid) {
            for mut msg in messages {
                msg.sender_name = response.name.clone();
                resolved.push(msg);
            }
        }

        debug!(
            "Resolved name for GUID {}: {} ({} pending messages)",
            response.guid,
            response.name,
            resolved.len()
        );

        Ok(resolved)
    }

    /// Build CMSG_NAME_QUERY packet.
    pub fn build_name_query(&self, guid: u64) -> NameQuery {
        NameQuery { guid }
    }

    /// Queue a chat message for name resolution, with bounded eviction.
    ///
    /// If this GUID already has pending messages, the new message is appended.
    /// If this is a new GUID and we're at capacity, the oldest GUID's pending
    /// messages are evicted (dropped) to make room.
    fn queue_pending_message(&mut self, guid: u64, msg: ChatMessage) {
        let is_new_guid = !self.pending_messages.contains_key(&guid);

        if is_new_guid && self.pending_messages.len() >= MAX_PENDING_GUIDS {
            // Evict the oldest entry
            if let Some(oldest_guid) = self.pending_message_order.pop_front() {
                let evicted = self.pending_messages.remove(&oldest_guid);
                self.pending_name_queries.remove(&oldest_guid);
                if let Some(msgs) = evicted {
                    warn!(
                        guid = oldest_guid,
                        count = msgs.len(),
                        "Evicted pending messages for GUID (name query never resolved)"
                    );
                }
            }
        }

        self.pending_messages.entry(guid).or_default().push(msg);

        if is_new_guid {
            self.pending_message_order.push_back(guid);
        }
    }

    /// Handle SMSG_CHANNEL_NOTIFY.
    pub fn handle_channel_notify(&self, mut payload: Bytes) -> Result<()> {
        let notify = ChannelNotify::decode(&mut payload)?;
        let desc = notify.description();

        // Log based on notification type
        match notify.notify_type {
            0 | 2 => info!("{}", desc),
            _ => warn!("{}", desc),
        }

        Ok(())
    }

    /// Build CMSG_JOIN_CHANNEL packet.
    pub fn build_join_channel(&self, channel_name: &str) -> JoinChannelWotLK {
        use super::chat::channel_ids;
        JoinChannelWotLK {
            channel_id: channel_ids::get_channel_id(channel_name),
            channel_name: channel_name.to_string(),
        }
    }

    /// Build CMSG_MESSAGECHAT packet.
    pub fn build_chat_message(
        &self,
        chat_type: u8,
        message: &str,
        target: Option<&str>,
    ) -> SendChatMessage {
        SendChatMessage {
            chat_type,
            language: self.language_id,
            target: target.map(|s| s.to_string()),
            message: message.to_string(),
        }
    }

    // =========================================================================
    // Guild handling
    // =========================================================================

    /// Handle SMSG_GUILD_QUERY response.
    pub fn handle_guild_query(&mut self, mut payload: Bytes) -> Result<()> {
        let response = GuildQueryResponse::decode(&mut payload)?;
        debug!(
            "Guild info received: {} ({} ranks)",
            response.name,
            response.ranks.len()
        );
        self.guild_info = Some(response);
        Ok(())
    }

    /// Handle SMSG_GUILD_ROSTER response.
    pub fn handle_guild_roster(&mut self, mut payload: Bytes) -> Result<()> {
        let roster = GuildRoster::decode(&mut payload)?;

        debug!(
            "Guild roster received: {} members, MOTD: {}",
            roster.members.len(),
            if roster.motd.is_empty() {
                "(none)"
            } else {
                &roster.motd
            }
        );

        self.guild_motd = if roster.motd.is_empty() {
            None
        } else {
            Some(roster.motd.clone())
        };

        // Convert to common types
        self.guild_roster.clear();
        for member in roster.members {
            let rank_name = self
                .guild_info
                .as_ref()
                .and_then(|info| info.ranks.get(member.rank as usize))
                .cloned()
                .unwrap_or_default();

            self.guild_roster
                .insert(member.guid, member.to_guild_member(&rank_name));
        }

        // Update last request timestamp
        self.last_roster_request = Some(std::time::Instant::now());

        // Roster generation for guild dashboard testing
        // Enable with: cargo run --features test_guild_dashboard
        #[cfg(feature = "test_guild_dashboard")]
        {
            self.guild_roster = generate_test_roster();
        }

        Ok(())
    }

    /// Check if guild roster should be updated (every 60 seconds).
    pub fn should_update_guild_roster(&self) -> bool {
        if self.guild_id == 0 {
            return false; // Not in a guild
        }

        match self.last_roster_request {
            None => true, // Never requested
            Some(last) => last.elapsed().as_secs() >= 60,
        }
    }

    /// Request guild roster update and update timestamp.
    pub fn request_guild_roster(&mut self) -> GuildRosterRequest {
        self.last_roster_request = Some(std::time::Instant::now());
        self.build_guild_roster_request()
    }

    /// Handle SMSG_GUILD_EVENT.
    /// Returns GuildEventInfo if this is a trackable guild event.
    pub fn handle_guild_event(&mut self, mut payload: Bytes) -> Result<Option<GuildEventInfo>> {
        let event = GuildEventPacket::decode(&mut payload)?;

        if event.strings.iter().all(|s| s.trim().is_empty()) {
            return Ok(None);
        }

        // Convert event type to GuildEvent enum
        let guild_event = GuildEvent::from_id(event.event_type);

        // Get config name from GuildEvent enum
        let event_name = match guild_event {
            Some(e) => e.config_name().to_string(),
            None => return Ok(None),
        };

        // For REMOVED events, the strings are swapped:
        // strings[0] = kicked player (target), strings[1] = kicker (user)
        // For MOTD events:
        // strings[0] = MOTD text (no player name in packet)
        // For all other events:
        // strings[0] = player who triggered the event
        let (player_name, target_name, rank_name) = match event.event_type {
            crate::protocol::game::guild::guild_events::GE_REMOVED => {
                let kicker = event.target_name().map(|s| s.to_string()); // strings[1]
                let kicked = event.player_name().map(|s| s.to_string()); // strings[0]
                (kicker, kicked, None)
            }
            crate::protocol::game::guild::guild_events::GE_MOTD => {
                // MOTD only has one string: the MOTD text itself
                // No player name is included in the packet
                let motd_text = event.player_name().map(|s| s.to_string()); // strings[0]
                (None, motd_text, None)
            }
            crate::protocol::game::guild::guild_events::GE_PROMOTED
            | crate::protocol::game::guild::guild_events::GE_DEMOTED => {
                let player = event.player_name().map(|s| s.to_string()); // strings[0]
                let target = event.target_name().map(|s| s.to_string()); // strings[1]
                let rank = event.rank_name().map(|s| s.to_string()); // strings[2]
                (player, target, rank)
            }
            _ => {
                let player = event.player_name().map(|s| s.to_string()); // strings[0]
                (player, None, None)
            }
        };

        // Skip events from self (except MOTD)
        if event.event_type != crate::protocol::game::guild::guild_events::GE_MOTD {
            if let Some(player) = &self.player {
                if let Some(ref name) = player_name {
                    if name.eq_ignore_ascii_case(&player.name) {
                        return Ok(None);
                    }
                }
            }
        }

        // Update MOTD if this is a MOTD event
        if let Some(motd) = event.motd() {
            self.guild_motd = Some(motd.to_string());
        }

        // For MOTD events, player_name can be None (no player in packet)
        // Use empty string in that case
        let player_name = match player_name {
            Some(name) => name,
            None if event.event_type == crate::protocol::game::guild::guild_events::GE_MOTD => {
                String::new()
            }
            None => return Ok(None),
        };

        Ok(Some(GuildEventInfo {
            event_name,
            player_name,
            target_name,
            rank_name,
            achievement_id: None,
        }))
    }

    /// Build CMSG_GUILD_QUERY packet.
    pub fn build_guild_query(&self, guild_id: u32) -> GuildQuery {
        GuildQuery { guild_id }
    }

    /// Build CMSG_GUILD_ROSTER packet.
    pub fn build_guild_roster_request(&self) -> GuildRosterRequest {
        GuildRosterRequest
    }

    /// Get count of online guildies (excluding self).
    pub fn get_online_guildies_count(&self) -> usize {
        self.guild_roster
            .values()
            .filter(|m| m.online && !m.name.eq_ignore_ascii_case(&self.character_name))
            .count()
    }

    /// Get formatted list of online guildies.
    pub fn get_online_guildies(&self) -> Vec<GuildMember> {
        let mut online: Vec<_> = self
            .guild_roster
            .values()
            .filter(|m| m.online && !m.name.eq_ignore_ascii_case(&self.character_name))
            .cloned()
            .collect();

        online.sort_by(|a, b| a.name.cmp(&b.name));
        online
    }

    /// Search for a guild member by name (case-insensitive).
    pub fn search_guild_member(&self, search_name: &str) -> Option<GuildMember> {
        let search_lower = search_name.to_lowercase();

        // Search in guild roster first
        if let Some(member) = self
            .guild_roster
            .values()
            .find(|m| m.name.to_lowercase() == search_lower)
        {
            return Some(member.clone());
        }

        // If player is online but not in guild roster (rare case)
        if let Some(ref player) = self.player {
            if player.name.to_lowercase() == search_lower {
                return Some(GuildMember {
                    guid: player.guid,
                    name: player.name.clone(),
                    level: player.level,
                    class: player.class,
                    rank: 0,
                    rank_name: String::new(),
                    zone_id: player.zone_id,
                    online: true,
                    last_logoff: 0.0,
                    note: String::new(),
                    officer_note: String::new(),
                });
            }
        }

        None
    }

    /// Get guild MOTD.
    pub fn get_guild_motd(&self) -> Option<&str> {
        self.guild_motd.as_deref()
    }

    /// Convert to common GuildInfo type.
    pub fn get_guild_info(&self) -> GuildInfo {
        GuildInfo {
            name: self
                .guild_info
                .as_ref()
                .map_or(String::new(), |g| g.name.clone()),
            motd: self.guild_motd.clone().unwrap_or_default(),
            info: String::new(),
            members: self.guild_roster.values().cloned().collect(),
        }
    }

    // =========================================================================
    // Server notification messages
    // =========================================================================

    /// Handle SMSG_NOTIFICATION packet.
    pub fn handle_notification(&self, mut payload: Bytes) -> Result<String> {
        use crate::protocol::game::chat::ServerNotification;
        use crate::protocol::packets::PacketDecode;

        let notification = ServerNotification::decode(&mut payload)?;
        info!("Server notification: {}", notification.message);
        Ok(notification.message)
    }

    /// Handle SMSG_MOTD packet.
    pub fn handle_motd(&mut self, mut payload: Bytes) -> Result<Option<String>> {
        use crate::protocol::game::chat::ServerMotd;
        use crate::protocol::packets::PacketDecode;

        let motd = ServerMotd::decode(&mut payload)?;
        let message = motd.to_message();

        if !message.is_empty() {
            info!("Server MOTD: {}", message);
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    /// Handle SMSG_GM_MESSAGECHAT packet.
    pub fn handle_gm_messagechat(&mut self, mut payload: Bytes) -> Result<Option<ChatMessage>> {
        use crate::protocol::game::chat::MessageChat;

        let msg = MessageChat::decode_(&mut payload, true)?;

        self.process_chat_message(msg)
    }

    /// Internal method to process chat messages (used by both regular and GM handlers).
    fn process_chat_message(
        &mut self,
        msg: crate::protocol::game::chat::MessageChat,
    ) -> Result<Option<ChatMessage>> {
        use crate::protocol::game::chat::chat_events;

        // Ignore messages from self (except system messages)
        if let Some(self_guid) = self.self_guid {
            if msg.sender_guid == self_guid && msg.chat_type != chat_events::CHAT_MSG_SYSTEM {
                debug!("Ignoring message from self");
                return Ok(None);
            }
        }

        // Handle CHAT_MSG_IGNORED - convert to WHISPER_INFORM with format
        // Note: This is handled in decode, but the message comes through here with custom content
        if msg.chat_type == chat_events::CHAT_MSG_WHISPER_INFORM && msg.message == "is ignoring you"
        {
            // Need to look up the player name for the GUID
            if let Some(player) = self.player_names.get(&msg.sender_guid) {
                return Ok(Some(ChatMessage {
                    chat_type: crate::common::types::ChatType::WhisperInform,
                    language: msg.language,
                    sender_guid: msg.sender_guid,
                    sender_name: player.name.clone(),
                    channel_name: None,
                    content: msg.message.clone(),
                    format: Some("%user %message.".to_string()),
                    achievement_id: msg.achievement_id,
                }));
            }
            // Queue for name resolution if we don't know the name
            let chat_msg = ChatMessage {
                chat_type: crate::common::types::ChatType::WhisperInform,
                language: msg.language,
                sender_guid: msg.sender_guid,
                sender_name: format!("Unknown-{}", msg.sender_guid),
                channel_name: None,
                content: msg.message.clone(),
                format: Some("%user %message.".to_string()),
                achievement_id: msg.achievement_id,
            };
            self.queue_pending_message(msg.sender_guid, chat_msg);
            return Ok(None);
        }

        // Handle system messages that are whisper-related (AFK/DND responses)
        if msg.chat_type == chat_events::CHAT_MSG_SYSTEM {
            let txt_lower = msg.message.to_lowercase();
            if txt_lower.contains("is away from keyboard")
                || txt_lower.contains("does not wish to be disturbed")
            {
                // Convert to WHISPER_INFORM with custom format
                return Ok(Some(ChatMessage {
                    chat_type: crate::common::types::ChatType::WhisperInform,
                    language: msg.language,
                    sender_guid: 0,
                    sender_name: String::new(),
                    channel_name: None,
                    content: msg.message.clone(),
                    format: Some("%message.".to_string()),
                    achievement_id: msg.achievement_id,
                }));
            }
        }

        // If sender GUID is 0, it's a system message - no name lookup needed
        if msg.sender_guid == 0 {
            return Ok(Some(msg.to_chat_message(String::new())));
        }

        // Look up sender name in cache
        if let Some(player) = self.player_names.get(&msg.sender_guid) {
            return Ok(Some(msg.to_chat_message(player.name.clone())));
        }

        // Queue message for name resolution
        let chat_msg = msg.to_chat_message(format!("Unknown-{}", msg.sender_guid));
        self.queue_pending_message(msg.sender_guid, chat_msg);

        // Return None to indicate name query is needed
        debug!("Sender {} not in cache, need name query", msg.sender_guid);
        Ok(None)
    }

    /// Handle SMSG_SERVER_MESSAGE packet.
    pub fn handle_server_message(&self, mut payload: Bytes) -> Result<String> {
        use crate::protocol::game::chat::ServerMessage;
        use crate::protocol::packets::PacketDecode;

        let msg = ServerMessage::decode(&mut payload)?;
        let formatted = msg.formatted_message();

        info!("Server message: {}", formatted);
        Ok(formatted)
    }

    /// Handle SMSG_CHAT_PLAYER_NOT_FOUND packet.
    /// Returns a ChatMessage with error format for relaying to Discord.
    pub fn handle_chat_player_not_found(&self, mut payload: Bytes) -> Result<Option<ChatMessage>> {
        let not_found = ChatPlayerNotFound::decode(&mut payload)?;
        debug!("Chat player not found: {}", not_found.player_name);

        // Create a message that will be sent to the WHISPER_INFORM channel
        Ok(Some(ChatMessage {
            chat_type: crate::common::types::ChatType::WhisperInform,
            language: 0,
            sender_guid: 0,
            sender_name: not_found.player_name,
            channel_name: None,
            content: String::new(),
            format: Some("No player named '%user' is currently playing.".to_string()),
            achievement_id: None,
        }))
    }

    /// Handle SMSG_INIT_WORLD_STATES.
    pub fn handle_init_world_states(&mut self) {
        // Reset the tried_to_sit flag so we can try sitting again in this session/zone
        self.tried_to_sit = false;
        debug!("Reset sit flag (SMSG_INIT_WORLD_STATES)");
    }

    /// Handle SMSG_INVALIDATE_PLAYER.
    /// Removes the player from the name cache when the server signals they're no longer valid.
    pub fn handle_invalidate_player(&mut self, payload: Bytes) -> Result<()> {
        use crate::protocol::game::packets::InvalidatePlayer;
        use crate::protocol::packets::PacketDecode;

        let packet = InvalidatePlayer::decode(&mut payload.clone())?;
        if self.player_names.remove(&packet.guid).is_some() {
            debug!(
                "Removed player {} from name cache (SMSG_INVALIDATE_PLAYER)",
                packet.guid
            );
        }
        Ok(())
    }

    /// Build CMSG_GAMEOBJ_USE packet.
    pub fn build_gameobj_use(&self, guid: u64) -> GameObjUse {
        GameObjUse { guid }
    }

    /// Handle SMSG_UPDATE_OBJECT.
    /// Returns Some(guid) if we should interact with an object (sit on chair).
    pub fn handle_update_object(
        &mut self,
        mut payload: Bytes,
        sit_enabled: bool,
    ) -> Result<Option<u64>> {
        if !sit_enabled || self.tried_to_sit {
            return Ok(None);
        }

        if payload.remaining() < 4 {
            return Err(anyhow!(
                "handle_update_object: need 4 bytes for block_count, have {}",
                payload.remaining()
            ));
        }
        let block_count = payload.get_u32_le();
        let mut closest_chair_guid = None;
        let mut min_distance_sq = f32::MAX;

        for _ in 0..block_count {
            if payload.remaining() < 1 {
                break;
            }
            let block_type = payload.get_u8();

            match block_type {
                0 => {
                    // UPDATETYPE_VALUES
                    let _guid = unpack_guid(&mut payload)?;
                    self.parse_update_fields(&mut payload)?;
                }
                1 => {
                    // UPDATETYPE_MOVEMENT
                    let _guid = unpack_guid(&mut payload)?;
                    self.parse_movement(&mut payload)?;
                }
                2 | 3 => {
                    // UPDATETYPE_CREATE_OBJECT, UPDATETYPE_CREATE_OBJECT2
                    let guid = unpack_guid(&mut payload)?;
                    if payload.remaining() < 1 {
                        return Err(anyhow!(
                            "handle_update_object: need 1 byte for obj_type, have 0"
                        ));
                    }
                    let obj_type = payload.get_u8();
                    let movement = self.parse_movement(&mut payload)?;
                    self.parse_update_fields(&mut payload)?;

                    // Check for self update
                    if (movement.flags & 0x1) == 0x1 {
                        // UPDATEFLAG_SELF
                        self.world_position = Some((movement.x, movement.y, movement.z));
                    }

                    // Check for chair
                    // GAMEOBJECT_TYPE_GENERIC, GAMEOBJECT_TYPE_CHAIR
                    if obj_type == 5 || obj_type == 7 {
                        if let Some((px, py, pz)) = self.world_position {
                            if close_to(movement.x, px, 2.0)
                                && close_to(movement.y, py, 2.0)
                                && close_to(movement.z, pz, 2.0)
                            {
                                // Calculate distance squared to find the closest one
                                let dx = movement.x - px;
                                let dy = movement.y - py;
                                let dz = movement.z - pz;
                                let dist_sq = dx * dx + dy * dy + dz * dz;

                                if dist_sq < min_distance_sq {
                                    min_distance_sq = dist_sq;
                                    closest_chair_guid = Some(guid);
                                    debug!(
                                        "Found closer chair (dist_sq={}) at {},{},{}",
                                        dist_sq, movement.x, movement.y, movement.z
                                    );
                                }
                            }
                        }
                    }
                }
                4 | 5 => {
                    // UPDATETYPE_OUT_OF_RANGE_OBJECTS, UPDATETYPE_NEAR_OBJECTS
                    if payload.remaining() < 4 {
                        return Err(anyhow!(
                            "handle_update_object: need 4 bytes for OUT_OF_RANGE count, have {}",
                            payload.remaining()
                        ));
                    }
                    let count = payload.get_u32_le();
                    for _ in 0..count {
                        unpack_guid(&mut payload)?;
                    }
                }
                _ => {
                    return Ok(None);
                }
            }
        }

        if let Some(guid) = closest_chair_guid {
            self.tried_to_sit = true;
            Ok(Some(guid))
        } else {
            Ok(None)
        }
    }

    fn parse_update_fields(&self, buf: &mut Bytes) -> Result<()> {
        if buf.remaining() < 1 {
            return Ok(());
        }
        let count = buf.get_u8();
        let mask_bytes = (count as usize) * 4;
        if buf.remaining() < mask_bytes {
            return Err(anyhow!(
                "parse_update_fields: need {} bytes for {} field masks, have {}",
                mask_bytes,
                count,
                buf.remaining()
            ));
        }
        let mut counts = Vec::with_capacity(count as usize);
        for _ in 0..count {
            counts.push(buf.get_u32_le());
        }

        for c in counts {
            // skip 4 bytes for each set bit
            let set_bits = c.count_ones() as usize;
            let field_bytes = set_bits * 4;
            if buf.remaining() < field_bytes {
                return Err(anyhow!(
                    "parse_update_fields: need {} bytes for {} field values, have {}",
                    field_bytes,
                    set_bits,
                    buf.remaining()
                ));
            }
            buf.advance(field_bytes);
        }
        Ok(())
    }

    fn parse_movement(&self, buf: &mut Bytes) -> Result<Movement> {
        // Scala: parseWorldObjectUpdateMovement
        let mut x = 0.0;
        let mut y = 0.0;
        let mut z = 0.0;

        if buf.remaining() < 2 {
            return Err(anyhow!(
                "parse_movement: need 2 bytes for flags, have {}",
                buf.remaining()
            ));
        }
        let flags = buf.get_u16_le(); // readChar.reverseBytes = LE

        if (flags & 0x20) == 0x20 {
            // UPDATEFLAG_LIVING
            // Base living block: flags2(4) + flags3(2) + time(4) + xyz(12) + orientation(4) = 26
            if buf.remaining() < 26 {
                return Err(anyhow!(
                    "parse_movement: LIVING block needs 26 bytes, have {}",
                    buf.remaining()
                ));
            }
            let flags2 = buf.get_u32_le(); // flags_
            let _flags3 = buf.get_u16_le(); // flags__ (readChar.reverseBytes)
            buf.advance(4); // time?

            x = buf.get_f32_le();
            y = buf.get_f32_le();
            z = buf.get_f32_le();
            buf.advance(4); // o

            if (flags2 & 0x200) == 0x200 {
                // MOVEMENTFLAG_ONTRANSPORT: guid + pos(16) + time(4) + seat(1) = 21 + guid
                unpack_guid(buf)?;
                let transport_bytes = 4 * 4 + 4 + 1; // 21
                if buf.remaining() < transport_bytes {
                    return Err(anyhow!(
                        "parse_movement: ONTRANSPORT needs {} bytes, have {}",
                        transport_bytes,
                        buf.remaining()
                    ));
                }
                buf.advance(4 * 4);
                buf.advance(4);
                buf.advance(1);
                if (_flags3 & 0x400) == 0x400 {
                    if buf.remaining() < 4 {
                        return Err(anyhow!(
                            "parse_movement: ONTRANSPORT extra needs 4 bytes, have {}",
                            buf.remaining()
                        ));
                    }
                    buf.advance(4);
                }
            }

            if (flags2 & 0x200000) == 0x200000 // MOVEMENTFLAG_SWIMMING
                || (flags2 & 0x2000000) == 0x2000000
                    // MOVEMENTFLAG_FLYING
                    || (_flags3 & 0x20) == 0x20
            {
                if buf.remaining() < 4 {
                    return Err(anyhow!(
                        "parse_movement: swim/fly pitch needs 4 bytes, have {}",
                        buf.remaining()
                    ));
                }
                buf.advance(4);
            }

            // timestamp
            if buf.remaining() < 4 {
                return Err(anyhow!(
                    "parse_movement: timestamp needs 4 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(4); // timestamp?

            if (flags2 & 0x1000) == 0x1000 {
                // MOVEMENTFLAG_FALLING: 4 floats = 16 bytes
                if buf.remaining() < 16 {
                    return Err(anyhow!(
                        "parse_movement: FALLING needs 16 bytes, have {}",
                        buf.remaining()
                    ));
                }
                buf.advance(4 * 4);
            }

            if (flags2 & 0x4000000) == 0x4000000 {
                // MOVEMENTFLAG_SPLINE_ELEVATION
                if buf.remaining() < 4 {
                    return Err(anyhow!(
                        "parse_movement: SPLINE_ELEVATION needs 4 bytes, have {}",
                        buf.remaining()
                    ));
                }
                buf.advance(4);
            }

            // 9 speed floats = 36 bytes
            if buf.remaining() < 36 {
                return Err(anyhow!(
                    "parse_movement: speeds need 36 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(9 * 4); // speeds

            if (flags2 & 0x8000000) == 0x8000000 {
                // MOVEMENTFLAG_SPLINE_ENABLED
                if buf.remaining() < 4 {
                    return Err(anyhow!(
                        "parse_movement: spline flags need 4 bytes, have {}",
                        buf.remaining()
                    ));
                }
                let flags_spline = buf.get_u32_le();
                if (flags_spline & 0x20000) == 0x20000 {
                    if buf.remaining() < 4 {
                        return Err(anyhow!(
                            "parse_movement: spline target needs 4 bytes, have {}",
                            buf.remaining()
                        ));
                    }
                    buf.advance(4);
                }
                if (flags_spline & 0x10000) == 0x10000 {
                    if buf.remaining() < 8 {
                        return Err(anyhow!(
                            "parse_movement: spline angle needs 8 bytes, have {}",
                            buf.remaining()
                        ));
                    }
                    buf.advance(8);
                }
                if (flags_spline & 0x8000) == 0x8000 {
                    if buf.remaining() < 12 {
                        return Err(anyhow!(
                            "parse_movement: spline facing needs 12 bytes, have {}",
                            buf.remaining()
                        ));
                    }
                    buf.advance(3 * 4);
                }
                // duration(4) + time_passed(4) + id(4) + points header(8) = 32
                if buf.remaining() < 32 {
                    return Err(anyhow!(
                        "parse_movement: spline base data needs 32 bytes, have {}",
                        buf.remaining()
                    ));
                }
                buf.advance(3 * 4);
                buf.advance(2 * 4);
                buf.advance(2 * 4);
                let splines = buf.get_u32_le();
                let spline_data = (splines as usize) * 12; // 3 floats per point
                if buf.remaining() < spline_data {
                    return Err(anyhow!(
                        "parse_movement: {} spline points need {} bytes, have {}",
                        splines,
                        spline_data,
                        buf.remaining()
                    ));
                }
                for _ in 0..splines {
                    buf.advance(3 * 4);
                }
                // end_point: type(1) + xyz(12) = 13
                if buf.remaining() < 13 {
                    return Err(anyhow!(
                        "parse_movement: spline end point needs 13 bytes, have {}",
                        buf.remaining()
                    ));
                }
                buf.advance(1);
                buf.advance(3 * 4);
            }
        } else {
            if (flags & 0x100) == 0x100 {
                // UPDATEFLAG_POSITION: guid + xyz(12) + extra(16) + unknown(4) = 32 + guid
                unpack_guid(buf)?;
                if buf.remaining() < 32 {
                    return Err(anyhow!(
                        "parse_movement: POSITION needs 32 bytes, have {}",
                        buf.remaining()
                    ));
                }
                x = buf.get_f32_le();
                y = buf.get_f32_le();
                z = buf.get_f32_le();
                buf.advance(4 * 4);
                buf.advance(4);
            } else if (flags & 0x40) == 0x40 {
                // UPDATEFLAG_STATIONARY_POSITION: xyz(12) + orientation(4) = 16
                if buf.remaining() < 16 {
                    return Err(anyhow!(
                        "parse_movement: STATIONARY_POSITION needs 16 bytes, have {}",
                        buf.remaining()
                    ));
                }
                x = buf.get_f32_le();
                y = buf.get_f32_le();
                z = buf.get_f32_le();
                buf.advance(4);
            }
        }

        if (flags & 0x8) == 0x8 {
            // UPDATEFLAG_HIGHGUID
            if buf.remaining() < 4 {
                return Err(anyhow!(
                    "parse_movement: HIGHGUID needs 4 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(4);
        }
        if (flags & 0x10) == 0x10 {
            // UPDATEFLAG_LOWGUID
            if buf.remaining() < 4 {
                return Err(anyhow!(
                    "parse_movement: LOWGUID needs 4 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(4);
        }
        if (flags & 0x4) == 0x4 {
            // UPDATEFLAG_HAS_TARGET
            unpack_guid(buf)?;
        }
        if (flags & 0x2) == 0x2 {
            // UPDATEFLAG_TRANSPORT
            if buf.remaining() < 4 {
                return Err(anyhow!(
                    "parse_movement: TRANSPORT needs 4 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(4);
        }
        if (flags & 0x80) == 0x80 {
            // UPDATEFLAG_VEHICLE
            if buf.remaining() < 8 {
                return Err(anyhow!(
                    "parse_movement: VEHICLE needs 8 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(2 * 4);
        }
        if (flags & 0x200) == 0x200 {
            // UPDATEFLAG_ROTATION
            if buf.remaining() < 8 {
                return Err(anyhow!(
                    "parse_movement: ROTATION needs 8 bytes, have {}",
                    buf.remaining()
                ));
            }
            buf.advance(8);
        }

        Ok(Movement { flags, x, y, z })
    }
}

#[derive(Debug)]
struct Movement {
    flags: u16,
    x: f32,
    y: f32,
    z: f32,
}

fn close_to(x: f32, y: f32, precision: f32) -> bool {
    (x - y).abs() < precision
}

fn unpack_guid(buf: &mut Bytes) -> Result<u64> {
    if buf.remaining() < 1 {
        return Err(anyhow!("unpack_guid: buffer empty, need mask byte"));
    }
    let mask = buf.get_u8();
    let needed = mask.count_ones() as usize;
    if buf.remaining() < needed {
        return Err(anyhow!(
            "unpack_guid: need {} GUID bytes for mask {:#04x}, have {}",
            needed,
            mask,
            buf.remaining()
        ));
    }
    let mut guid: u64 = 0;

    for i in 0..8 {
        if (mask & (1 << i)) != 0 {
            let byte = buf.get_u8();
            guid |= (byte as u64) << (i * 8);
        }
    }

    Ok(guid)
}

#[cfg(feature = "test_guild_dashboard")]
fn generate_test_roster() -> HashMap<u64, GuildMember> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let count = rng.gen_range(0..=200);

    info!("Generating test roster with {} members", count);
    let mut result: HashMap<u64, GuildMember> = HashMap::new();
    for i in 0..count {
        let guid = rng.gen::<u64>();
        let name = format!("{:03}Plr{}", i, i);
        let level = rng.gen_range(1..=80);
        let class_id = rng.gen_range(1..=11);
        let zone_id = rng.gen_range(1..=50);
        let online = true;
        let member = GuildMember {
            guid,
            name,
            level,
            class: crate::common::resources::Class::from_id(class_id),
            rank: 0,
            rank_name: String::new(),
            zone_id,
            online,
            last_logoff: 0.0,
            note: String::new(),
            officer_note: String::new(),
        };
        result.insert(guid, member);
    }
    result
}
