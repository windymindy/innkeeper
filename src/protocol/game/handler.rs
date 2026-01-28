//! Game packet handling logic.

use std::collections::HashMap;

use bytes::Bytes;
use sha1::{Digest, Sha1};
use tracing::{debug, error, info, warn};

use crate::common::error::ProtocolError;
use crate::common::types::{ChatMessage, GuildInfo, GuildMember, Player};
use crate::protocol::game::chat::{
    chat_events, get_language_for_race, ChannelNotify, JoinChannelWotLK, MessageChat, NameQuery,
    NameQueryResponse, SendChatMessage,
};
use crate::protocol::game::guild::{
    GuildEventPacket, GuildQuery, GuildQueryResponse, GuildRoster, GuildRosterRequest,
};
use crate::protocol::game::packets::{
    AuthChallenge, AuthResponse, AuthSession, CharEnum, CharacterInfo, LoginVerifyWorld, Ping,
    PlayerLogin, Pong,
};
use crate::protocol::packets::PacketDecode;

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
    pub fn handle_auth_challenge(
        &self,
        packet: AuthChallenge,
    ) -> Result<AuthSession, ProtocolError> {
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
    pub fn handle_auth_response(&self, packet: AuthResponse) -> Result<bool, ProtocolError> {
        match packet {
            AuthResponse::Success {
                billing_time_remaining,
                billing_flags,
                billing_time_rested,
                expansion,
            } => {
                info!("Game auth successful!");
                Ok(true)
            }
            AuthResponse::Failure(code) => {
                error!("Game auth failed with code: 0x{:02X}", code);
                Ok(false)
            }
        }
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
    pub fn handle_login_verify_world(
        &mut self,
        packet: LoginVerifyWorld,
    ) -> Result<(), ProtocolError> {
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

    // =========================================================================
    // Chat handling
    // =========================================================================

    /// Handle SMSG_MESSAGECHAT packet.
    pub fn handle_messagechat(
        &mut self,
        mut payload: Bytes,
    ) -> Result<Option<ChatMessage>, ProtocolError> {
        let msg = MessageChat::decode(&mut payload)?;

        // Ignore messages from self (except system messages)
        if let Some(self_guid) = self.self_guid {
            if msg.sender_guid == self_guid && msg.chat_type != chat_events::CHAT_MSG_SYSTEM {
                debug!("Ignoring message from self");
                return Ok(None);
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

    /// Handle SMSG_NAME_QUERY response.
    pub fn handle_name_query(
        &mut self,
        mut payload: Bytes,
    ) -> Result<Vec<ChatMessage>, ProtocolError> {
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
    pub fn handle_channel_notify(&self, mut payload: Bytes) -> Result<(), ProtocolError> {
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
    pub fn handle_guild_query(&mut self, mut payload: Bytes) -> Result<(), ProtocolError> {
        let response = GuildQueryResponse::decode(&mut payload)?;
        info!(
            "Guild info received: {} ({} ranks)",
            response.name,
            response.ranks.len()
        );
        self.guild_info = Some(response);
        Ok(())
    }

    /// Handle SMSG_GUILD_ROSTER response.
    pub fn handle_guild_roster(&mut self, mut payload: Bytes) -> Result<(), ProtocolError> {
        let roster = GuildRoster::decode(&mut payload)?;

        info!(
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
    pub fn handle_guild_event(
        &mut self,
        mut payload: Bytes,
    ) -> Result<Option<String>, ProtocolError> {
        let event = GuildEventPacket::decode(&mut payload)?;

        // Skip events from self (except MOTD)
        if event.event_type != crate::protocol::game::guild::guild_events::GE_MOTD {
            if let Some(player) = &self.player {
                if let Some(name) = event.player_name() {
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

        // Format notification
        let notification = event.format_notification();
        if let Some(ref msg) = notification {
            info!("Guild event: {}", msg);
        }

        Ok(notification)
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
}
