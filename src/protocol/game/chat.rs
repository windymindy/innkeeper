//! Chat message handling.
//!
//! Handles SMSG_MESSAGECHAT parsing and CMSG_MESSAGECHAT sending.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::common::error::ProtocolError;
use crate::common::types::{ChatMessage, ChatType, Guid};
use crate::protocol::packets::{PacketDecode, PacketEncode};

/// Chat events (message types) from WoW protocol.
#[allow(dead_code)]
pub mod chat_events {
    pub const CHAT_MSG_SAY: u8 = 0x00;
    pub const CHAT_MSG_PARTY: u8 = 0x01;
    pub const CHAT_MSG_RAID: u8 = 0x02;
    pub const CHAT_MSG_GUILD: u8 = 0x03;
    pub const CHAT_MSG_OFFICER: u8 = 0x04;
    pub const CHAT_MSG_YELL: u8 = 0x05;
    pub const CHAT_MSG_WHISPER: u8 = 0x06;
    pub const CHAT_MSG_WHISPER_INFORM: u8 = 0x07;
    pub const CHAT_MSG_REPLY: u8 = 0x08;
    pub const CHAT_MSG_EMOTE: u8 = 0x09;
    pub const CHAT_MSG_TEXT_EMOTE: u8 = 0x0A;
    pub const CHAT_MSG_SYSTEM: u8 = 0x0B;
    pub const CHAT_MSG_MONSTER_SAY: u8 = 0x0C;
    pub const CHAT_MSG_MONSTER_YELL: u8 = 0x0D;
    pub const CHAT_MSG_CHANNEL: u8 = 0x0E;
    pub const CHAT_MSG_RAID_LEADER: u8 = 0x27;
    pub const CHAT_MSG_RAID_WARNING: u8 = 0x28;
    pub const CHAT_MSG_RAID_BOSS_WHISPER: u8 = 0x29;
    pub const CHAT_MSG_RAID_BOSS_EMOTE: u8 = 0x2A;
    pub const CHAT_MSG_BATTLEGROUND: u8 = 0x2C;
    pub const CHAT_MSG_BATTLEGROUND_LEADER: u8 = 0x2D;
    pub const CHAT_MSG_ACHIEVEMENT: u8 = 0x30;
    pub const CHAT_MSG_GUILD_ACHIEVEMENT: u8 = 0x31;
}

/// Channel notification types.
#[allow(dead_code)]
pub mod chat_notify {
    pub const CHAT_YOU_JOINED_NOTICE: u8 = 0x00;
    pub const CHAT_YOU_LEFT_NOTICE: u8 = 0x01;
    pub const CHAT_WRONG_PASSWORD_NOTICE: u8 = 0x02;
    pub const CHAT_MUTED_NOTICE: u8 = 0x03;
    pub const CHAT_BANNED_NOTICE: u8 = 0x06;
    pub const CHAT_WRONG_FACTION_NOTICE: u8 = 0x08;
    pub const CHAT_INVALID_NAME_NOTICE: u8 = 0x09;
    pub const CHAT_NOT_MODERATED_NOTICE: u8 = 0x0A;
    pub const CHAT_THROTTLED_NOTICE: u8 = 0x0E;
    pub const CHAT_NOT_IN_AREA_NOTICE: u8 = 0x0F;
    pub const CHAT_NOT_IN_LFG_NOTICE: u8 = 0x10;
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
        }
    }
}

impl PacketDecode for MessageChat {
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 5 {
            return Err(ProtocolError::PacketTooShort {
                needed: 5,
                got: buf.remaining(),
            });
        }

        let chat_type = buf.get_u8();
        let language = buf.get_u32_le();

        // Addon messages have language -1, skip them
        if language == languages::LANG_ADDON {
            return Err(ProtocolError::InvalidPacket {
                message: "Addon message".to_string(),
            });
        }

        // For channel messages, read channel name first
        let channel_name = if chat_type == chat_events::CHAT_MSG_CHANNEL {
            let name = read_cstring(buf)?;
            // Skip player GUID for channel messages (4 bytes unknown field in WotLK)
            if buf.remaining() >= 4 {
                buf.advance(4);
            }
            Some(name)
        } else {
            None
        };

        // Sender GUID
        if buf.remaining() < 8 {
            return Err(ProtocolError::PacketTooShort {
                needed: 8,
                got: buf.remaining(),
            });
        }
        let sender_guid = buf.get_u64_le();

        // Some message types have a second guid block we need to skip
        // SAY, YELL have target_guid after sender_guid
        let target_guid = match chat_type {
            chat_events::CHAT_MSG_SAY | chat_events::CHAT_MSG_YELL => {
                if buf.remaining() >= 8 {
                    Some(buf.get_u64_le())
                } else {
                    None
                }
            }
            _ => None,
        };

        // Message length (includes null terminator)
        if buf.remaining() < 4 {
            return Err(ProtocolError::PacketTooShort {
                needed: 4,
                got: buf.remaining(),
            });
        }
        let message_length = buf.get_u32_le();

        // Read the message (length includes null terminator, so read length-1)
        let msg_len = if message_length > 0 {
            (message_length - 1) as usize
        } else {
            0
        };

        if buf.remaining() < msg_len {
            return Err(ProtocolError::PacketTooShort {
                needed: msg_len,
                got: buf.remaining(),
            });
        }

        let message_bytes = buf.copy_to_bytes(msg_len);
        let message = String::from_utf8(message_bytes.to_vec()).unwrap_or_else(|_| String::new());

        // Skip null terminator
        if buf.remaining() > 0 {
            buf.advance(1);
        }

        // Chat tag (if present)
        let chat_tag = if buf.remaining() > 0 { buf.get_u8() } else { 0 };

        Ok(MessageChat {
            chat_type,
            language,
            sender_guid,
            channel_name,
            target_guid,
            message_length,
            message,
            chat_tag,
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
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 2 {
            return Err(ProtocolError::PacketTooShort {
                needed: 2,
                got: buf.remaining(),
            });
        }

        let notify_type = buf.get_u8();
        let channel_name = read_cstring(buf)?;

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
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 8 {
            return Err(ProtocolError::PacketTooShort {
                needed: 8,
                got: buf.remaining(),
            });
        }

        let guid = buf.get_u64_le();
        let name = read_cstring(buf)?;
        let realm_name = read_cstring(buf)?;

        // WotLK sends 4-byte values for race, gender, class
        if buf.remaining() < 12 {
            return Err(ProtocolError::PacketTooShort {
                needed: 12,
                got: buf.remaining(),
            });
        }

        let race = buf.get_u32_le();
        let gender = buf.get_u32_le();
        let class = buf.get_u32_le();

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

/// Helper function to read a null-terminated C string from the buffer.
fn read_cstring(buf: &mut Bytes) -> Result<String, ProtocolError> {
    let mut bytes = Vec::new();
    while buf.remaining() > 0 {
        let b = buf.get_u8();
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).map_err(|e| ProtocolError::InvalidString {
        message: e.to_string(),
    })
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
