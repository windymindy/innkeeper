//! Chat message handling.
//!
//! Handles SMSG_MESSAGECHAT parsing and CMSG_MESSAGECHAT sending.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::common::types::{ChatMessage, ChatType, Guid};
use crate::protocol::packets::{
    read_cstring, PacketDecode, PacketEncode, MAX_CSTRING_LONG, MAX_CSTRING_SHORT,
};
use anyhow::{anyhow, Result};

/// Chat events (message types) from WoW protocol.
#[allow(dead_code)]
pub mod chat_events {
    pub const CHAT_MSG_SYSTEM: u8 = 0x00;
    pub const CHAT_MSG_SAY: u8 = 0x01;
    pub const CHAT_MSG_PARTY: u8 = 0x02;
    pub const CHAT_MSG_RAID: u8 = 0x03;
    pub const CHAT_MSG_GUILD: u8 = 0x04;
    pub const CHAT_MSG_OFFICER: u8 = 0x05;
    pub const CHAT_MSG_YELL: u8 = 0x06;
    pub const CHAT_MSG_WHISPER: u8 = 0x07;
    pub const CHAT_MSG_WHISPER_INFORM: u8 = 0x09;
    pub const CHAT_MSG_EMOTE: u8 = 0x0A;
    pub const CHAT_MSG_CHANNEL: u8 = 0x11;
    pub const CHAT_MSG_IGNORED: u8 = 0x19;
    pub const CHAT_MSG_RAID_LEADER: u8 = 0x27;
    pub const CHAT_MSG_RAID_WARNING: u8 = 0x28;
    pub const CHAT_MSG_PARTY_LEADER: u8 = 0x33;
    pub const CHAT_MSG_ACHIEVEMENT: u8 = 0x30;
    pub const CHAT_MSG_GUILD_ACHIEVEMENT: u8 = 0x31;
}

/// Channel notification types.
#[allow(dead_code)]
pub mod chat_notify {
    pub const CHAT_JOINED_NOTICE: u8 = 0x00;
    pub const CHAT_LEFT_NOTICE: u8 = 0x01;
    pub const CHAT_YOU_JOINED_NOTICE: u8 = 0x02;
    pub const CHAT_YOU_LEFT_NOTICE: u8 = 0x03;
    pub const CHAT_WRONG_PASSWORD_NOTICE: u8 = 0x04;
    pub const CHAT_NOT_MEMBER_NOTICE: u8 = 0x05;
    pub const CHAT_NOT_MODERATOR_NOTICE: u8 = 0x06;
    pub const CHAT_PASSWORD_CHANGED_NOTICE: u8 = 0x07;
    pub const CHAT_OWNER_CHANGED_NOTICE: u8 = 0x08;
    pub const CHAT_PLAYER_NOT_FOUND_NOTICE: u8 = 0x09;
    pub const CHAT_NOT_OWNER_NOTICE: u8 = 0x0A;
    pub const CHAT_CHANNEL_OWNER_NOTICE: u8 = 0x0B;
    pub const CHAT_MODE_CHANGE_NOTICE: u8 = 0x0C;
    pub const CHAT_ANNOUNCEMENTS_ON_NOTICE: u8 = 0x0D;
    pub const CHAT_ANNOUNCEMENTS_OFF_NOTICE: u8 = 0x0E;
    pub const CHAT_MODERATION_ON_NOTICE: u8 = 0x0F;
    pub const CHAT_MODERATION_OFF_NOTICE: u8 = 0x10;
    pub const CHAT_MUTED_NOTICE: u8 = 0x11;
    pub const CHAT_PLAYER_KICKED_NOTICE: u8 = 0x12;
    pub const CHAT_BANNED_NOTICE: u8 = 0x13;
    pub const CHAT_PLAYER_BANNED_NOTICE: u8 = 0x14;
    pub const CHAT_PLAYER_UNBANNED_NOTICE: u8 = 0x15;
    pub const CHAT_PLAYER_NOT_BANNED_NOTICE: u8 = 0x16;
    pub const CHAT_PLAYER_ALREADY_MEMBER_NOTICE: u8 = 0x17;
    pub const CHAT_INVITE_NOTICE: u8 = 0x18;
    pub const CHAT_INVITE_WRONG_FACTION_NOTICE: u8 = 0x19;
    pub const CHAT_WRONG_FACTION_NOTICE: u8 = 0x1A;
    pub const CHAT_INVALID_NAME_NOTICE: u8 = 0x1B;
    pub const CHAT_NOT_MODERATED_NOTICE: u8 = 0x1C;
    pub const CHAT_PLAYER_INVITED_NOTICE: u8 = 0x1D;
    pub const CHAT_PLAYER_INVITE_BANNED_NOTICE: u8 = 0x1E;
    pub const CHAT_THROTTLED_NOTICE: u8 = 0x1F;
    pub const CHAT_NOT_IN_AREA_NOTICE: u8 = 0x20;
    pub const CHAT_NOT_IN_LFG_NOTICE: u8 = 0x21;
    pub const CHAT_VOICE_ON_NOTICE: u8 = 0x22;
    pub const CHAT_VOICE_OFF_NOTICE: u8 = 0x23;
}

/// Language IDs for chat messages.
#[allow(dead_code)]
pub mod languages {
    pub const LANG_UNIVERSAL: u32 = 0;
    pub const LANG_ORCISH: u32 = 1;
    pub const LANG_DARNASSIAN: u32 = 2;
    pub const LANG_TAURAHE: u32 = 3;
    pub const LANG_DWARVISH: u32 = 6;
    pub const LANG_COMMON: u32 = 7;
    pub const LANG_DEMONIC: u32 = 8;
    pub const LANG_TITAN: u32 = 9;
    pub const LANG_THALASSIAN: u32 = 10;
    pub const LANG_DRACONIC: u32 = 11;
    pub const LANG_GNOMISH: u32 = 13;
    pub const LANG_TROLL: u32 = 14;
    pub const LANG_GUTTERSPEAK: u32 = 33;
    pub const LANG_DRAENEI: u32 = 35;
    pub const LANG_ADDON: u32 = 0xFFFFFFFF; // -1 as u32
}

/// Pre-defined channel IDs for standard WoW channels.
#[allow(dead_code)]
pub mod channel_ids {
    pub const GENERAL: u32 = 0x01;
    pub const TRADE: u32 = 0x02;
    pub const LOCAL_DEFENSE: u32 = 0x16;
    pub const WORLD_DEFENSE: u32 = 0x17;
    pub const GUILD_RECRUITMENT: u32 = 0x19; // TBC/WotLK
    pub const LOOKING_FOR_GROUP: u32 = 0x1A;
}

/// SMSG_MESSAGECHAT packet data.
#[derive(Debug, Clone)]
pub struct MessageChat {
    pub chat_type: u8,
    pub language: u32,
    pub sender_guid: Guid,
    pub channel_name: Option<String>,
    pub target_guid: Option<Guid>,
    pub message_length: u32,
    pub message: String,
    pub chat_tag: u8,
    /// Achievement ID for guild achievement messages (read after chat tag).
    pub achievement_id: Option<u32>,
}

impl MessageChat {
    /// Convert to common ChatMessage type.
    pub fn to_chat_message(&self, sender_name: String) -> ChatMessage {
        ChatMessage {
            chat_type: ChatType::from_id(self.chat_type).unwrap_or(ChatType::System),
            language: self.language,
            sender_guid: self.sender_guid,
            sender_name,
            channel_name: self.channel_name.clone(),
            content: self.message.clone(),
            format: None,
            achievement_id: self.achievement_id,
        }
    }

    /// Convert to common ChatMessage type with custom format.
    pub fn to_chat_message_with_format(
        &self,
        sender_name: String,
        format: Option<String>,
    ) -> ChatMessage {
        ChatMessage {
            chat_type: ChatType::from_id(self.chat_type).unwrap_or(ChatType::System),
            language: self.language,
            sender_guid: self.sender_guid,
            sender_name,
            channel_name: self.channel_name.clone(),
            content: self.message.clone(),
            format,
            achievement_id: self.achievement_id,
        }
    }
}

impl PacketDecode for MessageChat {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        Self::decode_(buf, false)
    }
}

impl MessageChat {
    pub fn decode_(buf: &mut Bytes, is_gm: bool) -> Result<Self> {
        if buf.remaining() < 13 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                13,
                buf.remaining()
            ));
        }

        let chat_type = buf.get_u8();
        let language = buf.get_u32_le();

        match chat_type {
            chat_events::CHAT_MSG_SYSTEM => {}
            chat_events::CHAT_MSG_SAY => {}
            chat_events::CHAT_MSG_PARTY => {}
            chat_events::CHAT_MSG_RAID => {}
            chat_events::CHAT_MSG_GUILD => {}
            chat_events::CHAT_MSG_OFFICER => {}
            chat_events::CHAT_MSG_YELL => {}
            chat_events::CHAT_MSG_WHISPER => {}
            chat_events::CHAT_MSG_WHISPER_INFORM => {}
            chat_events::CHAT_MSG_EMOTE => {}
            chat_events::CHAT_MSG_CHANNEL => {}
            chat_events::CHAT_MSG_IGNORED => {}
            chat_events::CHAT_MSG_RAID_LEADER => {}
            chat_events::CHAT_MSG_RAID_WARNING => {}
            chat_events::CHAT_MSG_PARTY_LEADER => {}
            chat_events::CHAT_MSG_ACHIEVEMENT => {}
            chat_events::CHAT_MSG_GUILD_ACHIEVEMENT => {}
            _ => {
                return Err(anyhow!("skip"));
            }
        }

        // Addon messages have language -1, skip them
        if language == languages::LANG_ADDON {
            return Err(anyhow!("skip"));
        }

        // Read sender GUID (8 bytes)
        let sender_guid = buf.get_u64_le();

        // CHAT_MSG_IGNORED has a simpler packet structure:
        // Just the GUID, convert to WHISPER_INFORM with custom format
        if chat_type == chat_events::CHAT_MSG_IGNORED {
            return Ok(MessageChat {
                chat_type: chat_events::CHAT_MSG_WHISPER_INFORM,
                language,
                sender_guid,
                channel_name: None,
                target_guid: None,
                message_length: 0,
                message: "is ignoring you".to_string(),
                chat_tag: 0,
                achievement_id: None,
            });
        }

        // Skip 4 bytes (unknown field after sender GUID)
        if buf.remaining() >= 4 {
            buf.advance(4);
        }

        // GM messages: skip 4 bytes + skip prefix string
        if is_gm {
            if buf.remaining() >= 4 {
                buf.advance(4);
            }
            // Skip GM prefix string
            read_cstring(buf, MAX_CSTRING_SHORT)?;
        }

        // For channel messages, read channel name
        let channel_name = if chat_type == chat_events::CHAT_MSG_CHANNEL {
            Some(read_cstring(buf, MAX_CSTRING_SHORT)?)
        } else {
            None
        };

        // Skip 8 bytes (guid again - appears twice in protocol)
        if buf.remaining() >= 8 {
            buf.advance(8);
        }

        // Message length (includes null terminator)
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }
        let message_length = buf.get_u32_le();

        // Read the message (length includes null terminator, so read length-1)
        let msg_len = if message_length > 0 {
            (message_length - 1) as usize
        } else {
            0
        };

        if buf.remaining() < msg_len {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                msg_len,
                buf.remaining()
            ));
        }

        let message_bytes = buf.copy_to_bytes(msg_len);
        let message = String::from_utf8_lossy(&message_bytes).to_string();

        // Skip null terminator
        if buf.remaining() > 0 {
            buf.advance(1);
        }

        // Skip chat tag (1 byte)
        if buf.remaining() > 0 {
            buf.advance(1);
        }

        // For guild achievement messages, read the achievement ID (4 bytes) after chat tag
        let achievement_id =
            if chat_type == chat_events::CHAT_MSG_GUILD_ACHIEVEMENT && buf.remaining() >= 4 {
                Some(buf.get_u32_le())
            } else {
                None
            };

        Ok(MessageChat {
            chat_type,
            language,
            sender_guid,
            channel_name,
            target_guid: None, // Not parsed in WotLK format
            message_length,
            message,
            chat_tag: 0, // Already skipped
            achievement_id,
        })
    }
}

/// CMSG_MESSAGECHAT packet for sending chat messages.
#[derive(Debug, Clone)]
pub struct SendChatMessage {
    pub chat_type: u8,
    pub language: u32,
    pub target: Option<String>,
    pub message: String,
}

impl PacketEncode for SendChatMessage {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32_le(self.chat_type as u32);
        buf.put_u32_le(self.language);

        // For whisper/channel, write target first
        if let Some(ref target) = self.target {
            buf.put_slice(target.as_bytes());
            buf.put_u8(0); // null terminator
        }

        // Write message
        buf.put_slice(self.message.as_bytes());
        buf.put_u8(0); // null terminator
    }
}

impl From<SendChatMessage> for crate::protocol::packets::Packet {
    fn from(msg: SendChatMessage) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        msg.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_MESSAGECHAT,
            buf.freeze(),
        )
    }
}

/// CMSG_JOIN_CHANNEL packet.
#[derive(Debug, Clone)]
pub struct JoinChannel {
    pub channel_id: u32,
    pub has_voice: u8,
    pub channel_name: String,
    pub password: String,
}

impl PacketEncode for JoinChannel {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32_le(self.channel_id);
        buf.put_u8(self.has_voice);
        buf.put_u8(0); // unknown byte

        // Channel name (null-terminated)
        buf.put_slice(self.channel_name.as_bytes());
        buf.put_u8(0);

        // Password (null-terminated, can be empty)
        buf.put_slice(self.password.as_bytes());
        buf.put_u8(0);
    }
}

/// WotLK-specific JoinChannel (simpler format).
#[derive(Debug, Clone)]
pub struct JoinChannelWotLK {
    pub channel_name: String,
}

impl PacketEncode for JoinChannelWotLK {
    fn encode(&self, buf: &mut BytesMut) {
        // WotLK format from Scala: just channel name + null + another null
        buf.put_slice(self.channel_name.as_bytes());
        buf.put_u8(0); // null terminator
        buf.put_u8(0); // password (empty)
    }
}

impl From<JoinChannelWotLK> for crate::protocol::packets::Packet {
    fn from(join: JoinChannelWotLK) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        join.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_JOIN_CHANNEL,
            buf.freeze(),
        )
    }
}

/// SMSG_CHANNEL_NOTIFY packet data.
#[derive(Debug, Clone)]
pub struct ChannelNotify {
    pub notify_type: u8,
    pub channel_name: String,
}

impl ChannelNotify {
    /// Get a human-readable description of the notification.
    pub fn description(&self) -> String {
        match self.notify_type {
            chat_notify::CHAT_YOU_JOINED_NOTICE => {
                format!("Joined channel: [{}]", self.channel_name)
            }
            chat_notify::CHAT_YOU_LEFT_NOTICE => {
                format!("Left channel: [{}]", self.channel_name)
            }
            chat_notify::CHAT_WRONG_PASSWORD_NOTICE => {
                format!("Wrong password for channel: {}", self.channel_name)
            }
            chat_notify::CHAT_MUTED_NOTICE => {
                format!(
                    "[{}] You do not have permission to speak",
                    self.channel_name
                )
            }
            chat_notify::CHAT_BANNED_NOTICE => {
                format!("[{}] You are banned from that channel", self.channel_name)
            }
            chat_notify::CHAT_WRONG_FACTION_NOTICE => {
                format!("Wrong faction for channel: {}", self.channel_name)
            }
            chat_notify::CHAT_INVALID_NAME_NOTICE => "Invalid channel name".to_string(),
            chat_notify::CHAT_THROTTLED_NOTICE => {
                format!("[{}] Message rate limited, please wait", self.channel_name)
            }
            chat_notify::CHAT_NOT_IN_AREA_NOTICE => format!(
                "[{}] You are not in the correct area for this channel",
                self.channel_name
            ),
            chat_notify::CHAT_NOT_IN_LFG_NOTICE => format!(
                "[{}] You must be queued in LFG to join this channel",
                self.channel_name
            ),
            _ => format!(
                "Channel notification {} for {}",
                self.notify_type, self.channel_name
            ),
        }
    }
}

impl PacketDecode for ChannelNotify {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 2 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                2,
                buf.remaining()
            ));
        }

        let notify_type = buf.get_u8();
        let channel_name = read_cstring(buf, MAX_CSTRING_SHORT)?;

        Ok(ChannelNotify {
            notify_type,
            channel_name,
        })
    }
}

/// CMSG_NAME_QUERY packet.
#[derive(Debug, Clone)]
pub struct NameQuery {
    pub guid: Guid,
}

impl PacketEncode for NameQuery {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u64_le(self.guid);
    }
}

impl From<NameQuery> for crate::protocol::packets::Packet {
    fn from(query: NameQuery) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        query.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_NAME_QUERY,
            buf.freeze(),
        )
    }
}

/// SMSG_NAME_QUERY_RESPONSE packet data.
#[derive(Debug, Clone)]
pub struct NameQueryResponse {
    pub guid: Guid,
    pub name: String,
    pub realm_name: String,
    pub race: u32,
    pub gender: u32,
    pub class: u32,
}

impl PacketDecode for NameQueryResponse {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        // WotLK uses packed GUID (variable length, 1-9 bytes)
        let guid = read_packed_guid(buf)?;

        // WotLK has a nameKnown byte before the name
        if buf.remaining() < 1 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                1,
                buf.remaining()
            ));
        }
        let name_known = buf.get_u8();

        let (name, realm_name, race, gender, class) = if name_known == 0 {
            // Name is known - read full data
            let name = read_cstring(buf, MAX_CSTRING_SHORT)?;
            let realm_name = read_cstring(buf, MAX_CSTRING_SHORT)?;

            // WotLK sends 1-byte values for race, gender, class (not 4-byte!)
            if buf.remaining() < 3 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    3,
                    buf.remaining()
                ));
            }

            let race = buf.get_u8() as u32;
            let gender = buf.get_u8() as u32;
            let class = buf.get_u8() as u32;

            (name, realm_name, race, gender, class)
        } else {
            // Name not known - use defaults
            ("UNKNOWN".to_string(), "".to_string(), 0, 0, 0xFF)
        };

        Ok(NameQueryResponse {
            guid,
            name,
            realm_name,
            race,
            gender,
            class,
        })
    }
}

/// Helper function to read a packed GUID (variable length, 1-9 bytes).
/// WoW uses packed GUIDs to save bandwidth - only non-zero bytes of the GUID are sent.
fn read_packed_guid(buf: &mut Bytes) -> Result<u64> {
    if buf.remaining() < 1 {
        return Err(anyhow!(
            "Packet too short: need {} bytes, got {}",
            1,
            buf.remaining()
        ));
    }

    let set = buf.get_u8();
    let mut result = 0u64;

    for i in 0..8 {
        let on_bit = 1 << i;
        if (set & on_bit) == on_bit {
            if buf.remaining() < 1 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    1,
                    buf.remaining()
                ));
            }
            let byte_val = buf.get_u8() as u64;
            result |= byte_val << (i * 8);
        }
    }

    Ok(result)
}

/// Get the language ID for a race (for sending messages).
pub fn get_language_for_race(race: u8) -> u32 {
    match race {
        // Alliance races
        1 => languages::LANG_COMMON,  // Human
        3 => languages::LANG_COMMON,  // Dwarf
        4 => languages::LANG_COMMON,  // Night Elf
        7 => languages::LANG_COMMON,  // Gnome
        11 => languages::LANG_COMMON, // Draenei
        // Horde races
        2 => languages::LANG_ORCISH,  // Orc
        5 => languages::LANG_ORCISH,  // Undead
        6 => languages::LANG_ORCISH,  // Tauren
        8 => languages::LANG_ORCISH,  // Troll
        10 => languages::LANG_ORCISH, // Blood Elf
        // Unknown - default to Common
        _ => languages::LANG_COMMON,
    }
}

// ============================================================================
// Server Notification Messages
// ============================================================================

/// SMSG_NOTIFICATION packet - Simple server notification message.
#[derive(Debug, Clone)]
pub struct ServerNotification {
    pub message: String,
}

impl PacketDecode for ServerNotification {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        let message = read_cstring(buf, MAX_CSTRING_LONG)?;
        Ok(ServerNotification { message })
    }
}

/// SMSG_MOTD packet - Server Message of the Day (multiple lines).
#[derive(Debug, Clone)]
pub struct ServerMotd {
    pub lines: Vec<String>,
}

/// SMSG_CHAT_PLAYER_NOT_FOUND packet - Player not found for whisper.
#[derive(Debug, Clone)]
pub struct ChatPlayerNotFound {
    pub player_name: String,
}

impl PacketDecode for ChatPlayerNotFound {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        let player_name = read_cstring(buf, MAX_CSTRING_SHORT)?;
        Ok(ChatPlayerNotFound { player_name })
    }
}

impl ServerMotd {
    /// Convert to a single formatted message.
    pub fn to_message(&self) -> String {
        self.lines.join("\n")
    }
}

impl PacketDecode for ServerMotd {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }

        let line_count = buf.get_u32_le() as usize;
        let mut lines = Vec::with_capacity(line_count);

        for _ in 0..line_count {
            lines.push(read_cstring(buf, MAX_CSTRING_LONG)?);
        }

        Ok(ServerMotd { lines })
    }
}

/// Server message types for SMSG_SERVER_MESSAGE.
#[allow(dead_code)]
pub mod server_message_types {
    pub const SERVER_MSG_SHUTDOWN_TIME: u32 = 1;
    pub const SERVER_MSG_RESTART_TIME: u32 = 2;
    pub const SERVER_MSG_STRING: u32 = 3;
    pub const SERVER_MSG_SHUTDOWN_CANCELLED: u32 = 4;
    pub const SERVER_MSG_RESTART_CANCELLED: u32 = 5;
    pub const SERVER_MSG_BATTLEGROUND_SHUTDOWN: u32 = 6;
    pub const SERVER_MSG_BATTLEGROUND_RESTART: u32 = 7;
    pub const SERVER_MSG_INSTANCE_SHUTDOWN: u32 = 8;
    pub const SERVER_MSG_INSTANCE_RESTART: u32 = 9;
}

/// SMSG_SERVER_MESSAGE packet - Server announcements (shutdowns, restarts, etc.).
#[derive(Debug, Clone)]
pub struct ServerMessage {
    pub message_type: u32,
    pub text: String,
}

impl ServerMessage {
    /// Get a formatted message based on the type.
    pub fn formatted_message(&self) -> String {
        match self.message_type {
            server_message_types::SERVER_MSG_SHUTDOWN_TIME => {
                format!("Server shutdown in {}", self.text)
            }
            server_message_types::SERVER_MSG_RESTART_TIME => {
                format!("Server restart in {}", self.text)
            }
            server_message_types::SERVER_MSG_SHUTDOWN_CANCELLED => {
                "Server shutdown cancelled.".to_string()
            }
            server_message_types::SERVER_MSG_RESTART_CANCELLED => {
                "Server restart cancelled.".to_string()
            }
            _ => self.text.clone(),
        }
    }
}

impl PacketDecode for ServerMessage {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }

        let message_type = buf.get_u32_le();
        let text = read_cstring(buf, MAX_CSTRING_LONG)?;

        Ok(ServerMessage { message_type, text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_chat_message_encode() {
        let msg = SendChatMessage {
            chat_type: chat_events::CHAT_MSG_GUILD,
            language: languages::LANG_COMMON,
            target: None,
            message: "Hello".to_string(),
        };

        let mut buf = BytesMut::new();
        msg.encode(&mut buf);

        // 4 bytes chat type + 4 bytes language + 5 bytes "Hello" + 1 null = 14
        assert_eq!(buf.len(), 14);
    }

    #[test]
    fn test_join_channel_wotlk_encode() {
        let join = JoinChannelWotLK {
            channel_name: "World".to_string(),
        };

        let mut buf = BytesMut::new();
        join.encode(&mut buf);

        // 5 bytes "World" + 1 null + 1 null = 7
        assert_eq!(buf.len(), 7);
    }
}
