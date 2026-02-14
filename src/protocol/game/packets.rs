//! Game server packet definitions.

use crate::protocol::packets::{PacketDecode, PacketEncode};
use anyhow::{anyhow, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};

// Addon info blob (Ascension specific)
pub const ADDON_INFO: [u8; 216] = hex_literal::hex!("9e020000789c75d2c14ec3300cc6f1f0145c780fce744853a5e542c319b9c9476a3571aa341d6cd7bdd19e107103c93dff2c5bfacb8fc6982ef1f54a357cbcf889714686b4f7de3ce4afa793f9e71542ba6cbe7111d53aaa23ea3a9565875b4bf864a4605938d3a20db10496a82e38508204aa1a953c523b95b86b0edf4dc1578c5b74a5a455c1a33d4ca4173ada61ab675c744c9765d265e3143a9259d55ed6055e3fd837e4a1f8196d2f8f255f8b2a6fc44105f75b54bfe738c3925084d6db9519fa13b84a01c3cc29ed310bea5fbbdf9ee30fe33bc901");

/// SMSG_AUTH_CHALLENGE packet.
#[derive(Debug, Clone)]
pub struct AuthChallenge {
    pub server_seed: u32,
}

impl PacketDecode for AuthChallenge {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 8 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                8,
                buf.remaining()
            ));
        }
        // Skip 4 bytes (1 byte type + 3 bytes padding?)
        // Scala code says: msg.byteBuf.skipBytes(4) // Skip WotLK-specific padding/nulls
        buf.advance(4);

        let server_seed = buf.get_u32(); // readInt in Netty is Big Endian
        Ok(AuthChallenge { server_seed })
    }
}

/// CMSG_AUTH_SESSION packet.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub build: u32,
    pub login_server_id: u32,
    pub account: String,
    pub login_server_type: u32,
    pub client_seed: u32,
    pub region_id: u32,
    pub battlegroup_id: u32,
    pub realm_id: u32,
    pub dos_response: u64,
    pub digest: [u8; 20],
}

impl PacketEncode for AuthSession {
    fn encode(&self, buf: &mut BytesMut) {
        // out.writeShortLE(0) // Initial placeholder for header? No, it's field "unknown" in Scala comments
        // Wait, Scala code: out.writeShortLE(0)
        // This is usually opcode or something but standard WotLK CMSG_AUTH_SESSION doesn't start with 0x0000.
        // Ah, in Scala GamePacketEncoder, it handles header.
        // But here in GamePacketHandlerWotLK.scala:
        // out.writeShortLE(0)
        // out.writeIntLE(WowChatConfig.getGameBuild)
        // ...
        // This looks like payload fields.

        // Let's follow Scala exactly.

        buf.put_u16_le(0); // unknown

        buf.put_u32_le(self.build);

        buf.put_u32_le(self.login_server_id);

        // Account name is null-terminated
        buf.put_slice(self.account.as_bytes());
        buf.put_u8(0);

        buf.put_u32(self.login_server_type); // writeInt (BE)

        buf.put_u32(self.client_seed); // writeInt (BE)

        buf.put_u32_le(self.region_id);
        buf.put_u32_le(self.battlegroup_id);
        buf.put_u32_le(self.realm_id);
        buf.put_u64_le(self.dos_response);

        buf.put_slice(&self.digest);

        // Addon info is hardcoded
        buf.put_slice(&ADDON_INFO);
    }
}

impl From<AuthSession> for crate::protocol::packets::Packet {
    fn from(auth: AuthSession) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        auth.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_AUTH_SESSION,
            buf.freeze(),
        )
    }
}

/// SMSG_AUTH_RESPONSE packet.
#[derive(Debug, Clone)]
pub enum AuthResponse {
    Success {
        billing_time_remaining: u32,
        billing_flags: u8,
        billing_time_rested: u32,
        expansion: u8,
    },
    Failure(u8),
}

impl PacketDecode for AuthResponse {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 1 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                1,
                buf.remaining()
            ));
        }

        let result = buf.get_u8();
        if result == 0x0C {
            // AUTH_OK
            if buf.remaining() < 10 {
                return Ok(AuthResponse::Success {
                    billing_time_remaining: 0,
                    billing_flags: 0,
                    billing_time_rested: 0,
                    expansion: 0,
                });
            }
            Ok(AuthResponse::Success {
                billing_time_remaining: buf.get_u32_le(),
                billing_flags: buf.get_u8(),
                billing_time_rested: buf.get_u32_le(),
                expansion: buf.get_u8(),
            })
        } else {
            Ok(AuthResponse::Failure(result))
        }
    }
}

/// Information about a character in character list.
#[derive(Debug, Clone)]
pub struct CharacterInfo {
    pub guid: u64,
    pub name: String,
    pub race: u8,
    pub class: u8,
    pub gender: u8,
    pub skin: u8,
    pub face: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub facial_hair: u8,
    pub level: u8,
    pub zone_id: u32,
    pub map_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub guild_id: u32,
    pub flags: u32,
    pub first_login: u8,
    pub pet_display_id: u32,
    pub pet_level: u32,
    pub pet_family: u32,
    // Equipment items (19 slots)
    // We don't really need them for a chat bridge
}

/// SMSG_CHAR_ENUM response.
#[derive(Debug, Clone)]
pub struct CharEnum {
    pub characters: Vec<CharacterInfo>,
}

impl PacketDecode for CharEnum {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 1 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                1,
                buf.remaining()
            ));
        }

        let count = buf.get_u8();
        let mut characters = Vec::with_capacity(count as usize);

        for _ in 0..count {
            if buf.remaining() < 8 {
                break;
            }
            let guid = buf.get_u64_le();
            let name = read_cstring(buf)?;
            let race = buf.get_u8();
            let class = buf.get_u8();
            let gender = buf.get_u8();
            let skin = buf.get_u8();
            let face = buf.get_u8();
            let hair_style = buf.get_u8();
            let hair_color = buf.get_u8();
            let facial_hair = buf.get_u8();
            let level = buf.get_u8();
            let zone_id = buf.get_u32_le();
            let map_id = buf.get_u32_le();
            let x = buf.get_f32_le();
            let y = buf.get_f32_le();
            let z = buf.get_f32_le();
            let guild_id = buf.get_u32_le();
            let flags = buf.get_u32_le();
            let first_login = buf.get_u8();
            let pet_display_id = buf.get_u32_le();
            let pet_level = buf.get_u32_le();
            let pet_family = buf.get_u32_le();

            // Skip inventory (19 * (4 + 1))
            for _ in 0..19 {
                if buf.remaining() >= 5 {
                    buf.advance(5);
                }
            }

            characters.push(CharacterInfo {
                guid,
                name,
                race,
                class,
                gender,
                skin,
                face,
                hair_style,
                hair_color,
                facial_hair,
                level,
                zone_id,
                map_id,
                x,
                y,
                z,
                guild_id,
                flags,
                first_login,
                pet_display_id,
                pet_level,
                pet_family,
            });
        }

        Ok(CharEnum { characters })
    }
}

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

/// CMSG_PLAYER_LOGIN packet.
#[derive(Debug, Clone)]
pub struct PlayerLogin {
    pub guid: u64,
}

impl PacketEncode for PlayerLogin {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u64_le(self.guid);
    }
}

impl From<PlayerLogin> for crate::protocol::packets::Packet {
    fn from(login: PlayerLogin) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        login.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_PLAYER_LOGIN,
            buf.freeze(),
        )
    }
}

/// SMSG_LOGIN_VERIFY_WORLD packet.
#[derive(Debug, Clone)]
pub struct LoginVerifyWorld {
    pub map_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub o: f32,
}

impl PacketDecode for LoginVerifyWorld {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 20 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                20,
                buf.remaining()
            ));
        }
        Ok(LoginVerifyWorld {
            map_id: buf.get_u32_le(),
            x: buf.get_f32_le(),
            y: buf.get_f32_le(),
            z: buf.get_f32_le(),
            o: buf.get_f32_le(),
        })
    }
}

/// CMSG_PING packet.
#[derive(Debug, Clone)]
pub struct Ping {
    pub sequence: u32,
    pub latency: u32,
}

impl PacketEncode for Ping {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32_le(self.sequence);
        buf.put_u32_le(self.latency);
    }
}

impl From<Ping> for crate::protocol::packets::Packet {
    fn from(ping: Ping) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        ping.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_PING,
            buf.freeze(),
        )
    }
}

/// SMSG_PONG packet.
#[derive(Debug, Clone)]
pub struct Pong {
    pub sequence: u32,
}

impl PacketDecode for Pong {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 4 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                4,
                buf.remaining()
            ));
        }
        Ok(Pong {
            sequence: buf.get_u32_le(),
        })
    }
}

/// CMSG_KEEP_ALIVE packet (empty payload - TBC/WotLK only).
#[derive(Debug, Clone, Default)]
pub struct KeepAlive;

impl From<KeepAlive> for crate::protocol::packets::Packet {
    fn from(_: KeepAlive) -> Self {
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_KEEP_ALIVE,
            Bytes::new(),
        )
    }
}

/// CMSG_CHAR_ENUM request packet (empty payload).
#[derive(Debug, Clone, Default)]
pub struct CharEnumRequest;

impl PacketEncode for CharEnumRequest {
    fn encode(&self, _buf: &mut BytesMut) {
        // Empty payload - just the opcode
    }
}

impl From<CharEnumRequest> for crate::protocol::packets::Packet {
    fn from(_req: CharEnumRequest) -> Self {
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_CHAR_ENUM,
            bytes::Bytes::new(),
        )
    }
}

/// CMSG_LOGOUT_REQUEST packet (empty payload).
#[derive(Debug, Clone, Default)]
pub struct LogoutRequest;

impl PacketEncode for LogoutRequest {
    fn encode(&self, _buf: &mut BytesMut) {
        // Empty payload - just the opcode
    }
}

impl From<LogoutRequest> for crate::protocol::packets::Packet {
    fn from(_req: LogoutRequest) -> Self {
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_LOGOUT_REQUEST,
            bytes::Bytes::new(),
        )
    }
}

/// SMSG_INIT_WORLD_STATES packet (empty/ignored payload).
#[derive(Debug, Clone, Default)]
pub struct InitWorldStates;

impl PacketDecode for InitWorldStates {
    type Error = anyhow::Error;

    fn decode(_buf: &mut Bytes) -> Result<Self, Self::Error> {
        // We just need to know it arrived
        Ok(InitWorldStates)
    }
}

/// SMSG_INVALIDATE_PLAYER packet.
/// Sent by the server to signal that a player is no longer valid/visible.
#[derive(Debug, Clone)]
pub struct InvalidatePlayer {
    pub guid: u64,
}

impl PacketDecode for InvalidatePlayer {
    type Error = anyhow::Error;

    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error> {
        if buf.remaining() < 8 {
            return Err(anyhow!(
                "SMSG_INVALIDATE_PLAYER packet too short: need {} bytes, got {}",
                8,
                buf.remaining()
            ));
        }
        Ok(InvalidatePlayer {
            guid: buf.get_u64_le(),
        })
    }
}

/// CMSG_GAMEOBJ_USE packet.
#[derive(Debug, Clone)]
pub struct GameObjUse {
    pub guid: u64,
}

impl PacketEncode for GameObjUse {
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u64_le(self.guid);
    }
}

impl From<GameObjUse> for crate::protocol::packets::Packet {
    fn from(game_obj_use: GameObjUse) -> Self {
        use bytes::BytesMut;
        let mut buf = BytesMut::new();
        game_obj_use.encode(&mut buf);
        crate::protocol::packets::Packet::new(
            crate::protocol::packets::opcodes::CMSG_GAMEOBJ_USE,
            buf.freeze(),
        )
    }
}
