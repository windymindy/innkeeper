//! Guild roster and event handling.
//!
//! Handles SMSG_GUILD_ROSTER, SMSG_GUILD_EVENT, and SMSG_GUILD_QUERY packets.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::common::error::ProtocolError;
use crate::common::types::{Guid, GuildEvent, GuildMember};
use crate::protocol::packets::{PacketDecode, PacketEncode};

/// Guild event IDs from the protocol.
#[allow(dead_code)]
pub mod guild_events {
    pub const GE_PROMOTION: u8 = 0x00;
    pub const GE_DEMOTION: u8 = 0x01;
    pub const GE_MOTD: u8 = 0x02;
    pub const GE_JOINED: u8 = 0x03;
    pub const GE_LEFT: u8 = 0x04;
    pub const GE_REMOVED: u8 = 0x05;
    pub const GE_LEADER_IS: u8 = 0x06;
    pub const GE_LEADER_CHANGED: u8 = 0x07;
    pub const GE_DISBANDED: u8 = 0x08;
    pub const GE_TABARD_CHANGE: u8 = 0x09;
    pub const GE_RANK_UPDATED: u8 = 0x0A;
    pub const GE_RANK_CREATED: u8 = 0x0B;
    pub const GE_RANK_DELETED: u8 = 0x0C;
    pub const GE_SIGNED_ON: u8 = 0x0C;
    pub const GE_SIGNED_OFF: u8 = 0x0D;
    pub const GE_BANK_BAG_SLOTS_CHANGED: u8 = 0x0E;
    pub const GE_BANK_TAB_PURCHASED: u8 = 0x0F;
    pub const GE_BANK_TAB_UPDATED: u8 = 0x10;
    pub const GE_BANK_MONEY_SET: u8 = 0x11;
    pub const GE_BANK_TAB_AND_MONEY_UPDATED: u8 = 0x12;
    pub const GE_BANK_TEXT_CHANGED: u8 = 0x13;
}

/// Parsed guild information from SMSG_GUILD_QUERY.
#[derive(Debug, Clone, Default)]
pub struct GuildQueryResponse {
    pub guild_id: u32,
    pub name: String,
    pub ranks: Vec<String>,
    pub emblem_style: u32,
    pub emblem_color: u32,
    pub border_style: u32,
    pub border_color: u32,
    pub background_color: u32,
}

impl PacketDecode for GuildQueryResponse {
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(ProtocolError::PacketTooShort {
                needed: 4,
                got: buf.remaining(),
            });
        }

        let guild_id = buf.get_u32_le();
        let name = read_cstring(buf)?;

        // Read up to 10 rank names
        let mut ranks = Vec::with_capacity(10);
        for _ in 0..10 {
            let rank_name = read_cstring(buf)?;
            if !rank_name.is_empty() {
                ranks.push(rank_name);
            }
        }

        // Skip emblem data if present (5 * 4 bytes)
        let emblem_style = if buf.remaining() >= 4 {
            buf.get_u32_le()
        } else {
            0
        };
        let emblem_color = if buf.remaining() >= 4 {
            buf.get_u32_le()
        } else {
            0
        };
        let border_style = if buf.remaining() >= 4 {
            buf.get_u32_le()
        } else {
            0
        };
        let border_color = if buf.remaining() >= 4 {
            buf.get_u32_le()
        } else {
            0
        };
        let background_color = if buf.remaining() >= 4 {
            buf.get_u32_le()
        } else {
            0
        };

        Ok(GuildQueryResponse {
            guild_id,
            name,
            ranks,
            emblem_style,
            emblem_color,
            border_style,
            border_color,
            background_color,
        })
    }
}

/// CMSG_GUILD_QUERY packet.
#[derive(Debug, Clone)]
pub struct GuildQuery {
    pub guild_id: u32,
}

impl PacketEncode for GuildQuery {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32_le(self.guild_id);
    }
}

/// CMSG_GUILD_ROSTER packet (empty payload).
#[derive(Debug, Clone)]
pub struct GuildRosterRequest;

impl PacketEncode for GuildRosterRequest {
    fn encode(&self, _buf: &mut BytesMut) {
        // Empty packet
    }
}

/// A single guild member entry from SMSG_GUILD_ROSTER.
#[derive(Debug, Clone)]
pub struct GuildRosterMember {
    pub guid: Guid,
    pub online: bool,
    pub name: String,
    pub rank: u32,
    pub level: u8,
    pub class: u8,
    pub gender: u8,
    pub zone_id: u32,
    pub last_logoff: f32,
    pub public_note: String,
    pub officer_note: String,
}

impl GuildRosterMember {
    /// Convert to common GuildMember type.
    pub fn to_guild_member(&self, rank_name: &str) -> GuildMember {
        GuildMember {
            guid: self.guid,
            name: self.name.clone(),
            level: self.level,
            class: crate::common::resources::Class::from_id(self.class),
            rank: self.rank as u8,
            rank_name: rank_name.to_string(),
            zone_id: self.zone_id,
            online: self.online,
            note: self.public_note.clone(),
            officer_note: self.officer_note.clone(),
        }
    }
}

/// SMSG_GUILD_ROSTER response.
#[derive(Debug, Clone, Default)]
pub struct GuildRoster {
    pub member_count: u32,
    pub motd: String,
    pub guild_info: String,
    pub rank_count: u32,
    pub ranks: Vec<u32>, // Rights flags for each rank
    pub members: Vec<GuildRosterMember>,
}

impl PacketDecode for GuildRoster {
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(ProtocolError::PacketTooShort {
                needed: 4,
                got: buf.remaining(),
            });
        }

        let member_count = buf.get_u32_le();
        let motd = read_cstring(buf)?;
        let guild_info = read_cstring(buf)?;

        if buf.remaining() < 4 {
            return Err(ProtocolError::PacketTooShort {
                needed: 4,
                got: buf.remaining(),
            });
        }

        let rank_count = buf.get_u32_le();

        // Read rank rights
        let mut ranks = Vec::with_capacity(rank_count as usize);
        for _ in 0..rank_count {
            if buf.remaining() >= 4 {
                ranks.push(buf.get_u32_le());
            }
        }

        // Read members
        let mut members = Vec::with_capacity(member_count as usize);
        for _ in 0..member_count {
            if buf.remaining() < 9 {
                break;
            }

            let guid = buf.get_u64_le();
            let online = buf.get_u8() != 0;
            let name = read_cstring(buf)?;

            if buf.remaining() < 4 {
                break;
            }
            let rank = buf.get_u32_le();

            if buf.remaining() < 3 {
                break;
            }
            let level = buf.get_u8();
            let class = buf.get_u8();
            let gender = buf.get_u8();

            if buf.remaining() < 4 {
                break;
            }
            let zone_id = buf.get_u32_le();

            // Last logoff only present for offline members
            let last_logoff = if !online && buf.remaining() >= 4 {
                buf.get_f32_le()
            } else {
                0.0
            };

            let public_note = read_cstring(buf)?;
            let officer_note = read_cstring(buf)?;

            members.push(GuildRosterMember {
                guid,
                online,
                name,
                rank,
                level,
                class,
                gender,
                zone_id,
                last_logoff,
                public_note,
                officer_note,
            });
        }

        Ok(GuildRoster {
            member_count,
            motd,
            guild_info,
            rank_count,
            ranks,
            members,
        })
    }
}

/// SMSG_GUILD_EVENT parsed data.
#[derive(Debug, Clone)]
pub struct GuildEventPacket {
    pub event_type: u8,
    pub strings: Vec<String>,
}

impl GuildEventPacket {
    /// Convert to the common GuildEvent type if applicable.
    pub fn to_guild_event(&self) -> Option<GuildEvent> {
        GuildEvent::from_id(self.event_type)
    }

    /// Get the primary affected player name (if any).
    pub fn player_name(&self) -> Option<&str> {
        self.strings.first().map(|s| s.as_str())
    }

    /// Get the MOTD text (for GE_MOTD events).
    pub fn motd(&self) -> Option<&str> {
        if self.event_type == guild_events::GE_MOTD {
            self.strings.first().map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Format the event as a notification message.
    pub fn format_notification(&self) -> Option<String> {
        let event = self.to_guild_event()?;
        let strings = &self.strings;

        if strings.is_empty() {
            return None;
        }

        Some(match event {
            GuildEvent::Promotion => {
                if strings.len() >= 3 {
                    format!(
                        "{} has promoted {} to {}",
                        strings[0], strings[1], strings[2]
                    )
                } else {
                    return None;
                }
            }
            GuildEvent::Demotion => {
                if strings.len() >= 3 {
                    format!(
                        "{} has demoted {} to {}",
                        strings[0], strings[1], strings[2]
                    )
                } else {
                    return None;
                }
            }
            GuildEvent::Motd => {
                format!("Guild MOTD: {}", strings[0])
            }
            GuildEvent::Joined => {
                format!("{} has joined the guild", strings[0])
            }
            GuildEvent::Left => {
                format!("{} has left the guild", strings[0])
            }
            GuildEvent::Removed => {
                if strings.len() >= 2 {
                    format!(
                        "{} has been kicked from the guild by {}",
                        strings[0], strings[1]
                    )
                } else {
                    format!("{} has been removed from the guild", strings[0])
                }
            }
            GuildEvent::SignedOn => {
                format!("{} has come online", strings[0])
            }
            GuildEvent::SignedOff => {
                format!("{} has gone offline", strings[0])
            }
        })
    }
}

impl PacketDecode for GuildEventPacket {
    type Error = ProtocolError;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 2 {
            return Err(ProtocolError::PacketTooShort {
                needed: 2,
                got: buf.remaining(),
            });
        }

        let event_type = buf.get_u8();
        let num_strings = buf.get_u8();

        let mut strings = Vec::with_capacity(num_strings as usize);
        for _ in 0..num_strings {
            strings.push(read_cstring(buf)?);
        }

        Ok(GuildEventPacket {
            event_type,
            strings,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guild_query_encode() {
        let query = GuildQuery { guild_id: 12345 };
        let mut buf = BytesMut::new();
        query.encode(&mut buf);

        assert_eq!(buf.len(), 4);
        assert_eq!(buf[0..4], 12345u32.to_le_bytes());
    }

    #[test]
    fn test_guild_event_format() {
        let event = GuildEventPacket {
            event_type: guild_events::GE_JOINED,
            strings: vec!["TestPlayer".to_string()],
        };

        let formatted = event.format_notification();
        assert_eq!(
            formatted,
            Some("TestPlayer has joined the guild".to_string())
        );
    }
}
