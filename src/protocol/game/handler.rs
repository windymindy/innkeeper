//! Game packet handling logic.

use std::collections::HashMap;

use bytes::Bytes;
use sha1::{Digest, Sha1};
use tracing::{debug, error, info, warn};

use crate::common::types::{ChatMessage, GuildEvent, GuildInfo, GuildMember, Player};
use crate::protocol::game::chat::{
    get_language_for_race, ChannelNotify, ChatPlayerNotFound, JoinChannelWotLK, MessageChat,
    NameQuery, NameQueryResponse, SendChatMessage,
};
use crate::protocol::game::guild::{
    GuildEventPacket, GuildQuery, GuildQueryResponse, GuildRoster, GuildRosterRequest,
};
use crate::protocol::game::packets::{
    AuthChallenge, AuthResponse, AuthSession, CharEnum, CharEnumRequest, CharacterInfo,
    LoginVerifyWorld, Ping, PlayerLogin, Pong,
};
use crate::protocol::packets::PacketDecode;
use anyhow::Result;

/// Game protocol handler state.
pub struct GameHandler {
    account: String,
    session_key: [u8; 40],
    realm_id: u32,
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

    /// Cache of player names by GUID
    pub player_names: HashMap<u64, Player>,
    /// Pending chat messages waiting for name resolution
    pub pending_messages: HashMap<u64, Vec<ChatMessage>>,
}

impl GameHandler {
    pub fn new(account: &str, session_key: [u8; 40], realm_id: u32) -> Self {
        Self {
            account: account.to_string(),
            session_key,
            realm_id,
            player: None,
            self_guid: None,
            guild_id: 0,
            language_id: 7, // Default to Common
            in_world: false,
            guild_info: None,
            guild_roster: HashMap::new(),
            guild_motd: None,
            player_names: HashMap::new(),
            pending_messages: HashMap::new(),
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

    /// Build CMSG_LOGOUT_REQUEST.
    pub fn build_logout_request(&self) -> crate::protocol::game::packets::LogoutRequest {
        crate::protocol::game::packets::LogoutRequest
    }

    // =========================================================================
    // Chat handling
    // =========================================================================

    /// Handle SMSG_MESSAGECHAT packet.
    pub fn handle_messagechat(&mut self, mut payload: Bytes) -> Result<Option<ChatMessage>> {
        let msg = match MessageChat::decode(&mut payload) {
            Ok(msg) => msg,
            Err(e) if e.to_string().contains("skip") => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };

        self.process_chat_message(msg)
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

    /// Handle SMSG_CHANNEL_NOTIFY.
    pub fn handle_channel_notify(&self, mut payload: Bytes) -> Result<()> {
        let notify = ChannelNotify::decode(&mut payload)?;
        let desc = notify.description();

        // Log based on notification type
        match notify.notify_type {
            0 => info!("{}", desc),
            _ => warn!("{}", desc),
        }

        Ok(())
    }

    /// Build CMSG_JOIN_CHANNEL packet.
    pub fn build_join_channel(&self, channel_name: &str) -> JoinChannelWotLK {
        JoinChannelWotLK {
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

        Ok(())
    }

    /// Handle SMSG_GUILD_EVENT.
    /// Returns (player_name, event_name) if this is a trackable guild event.
    pub fn handle_guild_event(&mut self, mut payload: Bytes) -> Result<Option<(String, String)>> {
        let event = GuildEventPacket::decode(&mut payload)?;

        // Get player name
        let player_name = event.player_name().map(|s| s.to_string());

        // Convert event type to GuildEvent enum
        let guild_event = GuildEvent::from_id(event.event_type);

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

        // Get config name from GuildEvent enum
        let event_name = guild_event.map(|e| e.config_name().to_string());

        match (player_name, event_name) {
            (Some(name), Some(event)) => Ok(Some((name, event))),
            _ => Ok(None),
        }
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
        let self_name = self.player.as_ref().map(|p| p.name.as_str());

        self.guild_roster
            .values()
            .filter(|m| {
                m.online && self_name.map_or(true, |name| !m.name.eq_ignore_ascii_case(name))
            })
            .count()
    }

    /// Get formatted list of online guildies.
    pub fn get_online_guildies(&self) -> String {
        let self_name = self.player.as_ref().map(|p| p.name.as_str());

        let mut online: Vec<_> = self
            .guild_roster
            .values()
            .filter(|m| {
                m.online && self_name.map_or(true, |name| !m.name.eq_ignore_ascii_case(name))
            })
            .collect();

        online.sort_by(|a, b| a.name.cmp(&b.name));

        if online.is_empty() {
            return "No guildies online.".to_string();
        }

        let lines: Vec<String> = online
            .iter()
            .map(|m| {
                let class_name = m.class.map_or("Unknown", |c| c.name());
                format!("{} (Level {} {})", m.name, m.level, class_name)
            })
            .collect();

        format!(
            "{} guildie{} online:\n{}",
            online.len(),
            if online.len() == 1 { "" } else { "s" },
            lines.join(", ")
        )
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
            };
            self.pending_messages
                .entry(msg.sender_guid)
                .or_default()
                .push(chat_msg);
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
        self.pending_messages
            .entry(msg.sender_guid)
            .or_default()
            .push(chat_msg.clone());

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
        }))
    }
}
