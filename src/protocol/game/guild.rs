//! Guild roster and event handling.
//!
//! Handles SMSG_GUILD_ROSTER, SMSG_GUILD_EVENT, and SMSG_GUILD_QUERY packets.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::common::types::{Guid, GuildEvent, GuildMember};
use crate::protocol::packets::{PacketDecode, PacketEncode};
use anyhow::{anyhow, Result};

/// Guild event IDs from the protocol.
/// WotLK values
#[allow(dead_code)]
pub mod guild_events {
    pub const GE_PROMOTED: u8 = 0x00;
    pub const GE_DEMOTED: u8 = 0x01;
    pub const GE_MOTD: u8 = 0x02;
    pub const GE_JOINED: u8 = 0x03;
    pub const GE_LEFT: u8 = 0x04;
    pub const GE_REMOVED: u8 = 0x05;
    pub const GE_SIGNED_ON: u8 = 0x0C;
    pub const GE_SIGNED_OFF: u8 = 0x0D;
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
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
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

impl From<GuildQuery> for crate::protocol::packets::Packet {
    fn from(query: GuildQuery) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        query.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_GUILD_QUERY,
            buf.freeze(),
        )
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

impl From<GuildRosterRequest> for crate::protocol::packets::Packet {
    fn from(_req: GuildRosterRequest) -> Self {
        crate::protocol::packets::Packet::empty(
            crate::protocol::packets::opcodes::CMSG_GUILD_ROSTER,
        )
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
    pub members: Vec<GuildRosterMember>,
}

impl PacketDecode for GuildRoster {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }

        let member_count = buf.get_u32_le();
        let motd = read_cstring(buf)?;
        let guild_info = read_cstring(buf)?;

        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }
        let rank_count = buf.get_u32_le();
        if buf.remaining() < (8 + 48) * (rank_count as usize) {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                (8 + 48) * rank_count,
                buf.remaining()
            ));
        }

        buf.advance((8 + 48) * (rank_count as usize));

        // Read members
        let mut members = Vec::with_capacity(member_count as usize);
        for i in 0..member_count {
            if buf.remaining() < 9 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    9,
                    buf.remaining()
                ));
            }
            let guid = buf.get_u64_le();
            let online = buf.get_u8() != 0;

            if buf.remaining() < 1 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    1,
                    buf.remaining()
                ));
            }
            let name = read_cstring(buf)?;

            // Skip guild rank (4 bytes) - not stored
            if buf.remaining() < 11 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    11,
                    buf.remaining()
                ));
            }
            buf.advance(4);
            let level = buf.get_u8();
            let class = buf.get_u8();
            buf.advance(1);
            let zone_id = buf.get_u32_le();

            // Last logoff only present for offline members
            if !online && buf.remaining() < 4 {
                return Err(anyhow!(
                    "Packet too short: need {} bytes, got {}",
                    4,
                    buf.remaining()
                ));
            }
            let last_logoff = if !online { buf.get_f32_le() } else { 0.0 };

            // Skip public and officer notes (strings)
            let _public_note = read_cstring(buf)?;
            let _officer_note = read_cstring(buf)?;

            members.push(GuildRosterMember {
                guid,
                online,
                name,
                rank: 0, // Not stored, skipped
                level,
                class,
                gender: 0, // Not present in packet
                zone_id,
                last_logoff,
                public_note: String::new(),
                officer_note: String::new(),
            });
        }

        if members.len() != member_count as usize {
            tracing::warn!(
                "Guild roster parsed only {}/{} members",
                members.len(),
                member_count
            );
        }

        Ok(GuildRoster {
            member_count,
            motd,
            guild_info,
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

    /// Get the target player name for promotion/demotion events.
    pub fn target_name(&self) -> Option<&str> {
        self.strings.get(1).map(|s| s.as_str())
    }

    /// Get the rank name for promotion/demotion events.
    pub fn rank_name(&self) -> Option<&str> {
        self.strings.get(2).map(|s| s.as_str())
    }

    /// Get the MOTD text (for GE_MOTD events).
    pub fn motd(&self) -> Option<&str> {
        if self.event_type == guild_events::GE_MOTD {
            self.strings.first().map(|s| s.as_str())
        } else {
            None
        }
    }
}

impl PacketDecode for GuildEventPacket {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 2 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                2,
                buf.remaining()
            ));
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
fn read_cstring(buf: &mut Bytes) -> Result<String> {
    let mut bytes = Vec::new();
    while buf.remaining() > 0 {
        let b = buf.get_u8();
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
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
}
