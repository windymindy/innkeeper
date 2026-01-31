//! Game server TCP connection and codec.

use bytes::{Buf, BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::protocol::game::header::GameHeaderCrypt;
use crate::protocol::packets::Packet;
use anyhow::Error;

/// Codec for WoW game server packets.
pub struct GamePacketCodec {
    header_crypt: GameHeaderCrypt,
}

impl GamePacketCodec {
    pub fn new(header_crypt: GameHeaderCrypt) -> Self {
        Self { header_crypt }
    }

    /// Initialize header encryption with session key.
    pub fn init_crypt(&mut self, session_key: &[u8]) {
        self.header_crypt.init(session_key);
    }
}

impl Decoder for GamePacketCodec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(Option::None);
        }

        // Server to Client header is 2-3 bytes size + 2 bytes opcode
        // Check first bit of first byte for large packet (size > 0x7FFF)
        let large = (src[0] & 0x80) != 0;
        let header_size = if large { 5 } else { 4 };

        if src.len() < header_size {
            return Ok(Option::None);
        }

        // Copy header to decrypt
        let mut header = [0u8; 5];
        header[..header_size].copy_from_slice(&src[..header_size]);
        self.header_crypt.decrypt(&mut header[..header_size]);

        let (payload_size, opcode) = if large {
            let size = (((header[0] & 0x7F) as usize) << 16)
                | ((header[1] as usize) << 8)
                | (header[2] as usize);
            let op = u16::from_le_bytes([header[3], header[4]]);
            (size - 2, op)
        } else {
            let size = ((header[0] as usize) << 8) | (header[1] as usize);
            let op = u16::from_le_bytes([header[2], header[3]]);
            (size - 2, op)
        };

        if src.len() < header_size + payload_size {
            return Ok(Option::None);
        }

        // Advance buffer past header
        src.advance(header_size);

        // Extract payload
        let payload = src.split_to(payload_size).freeze();

        Ok(Some(Packet { opcode, payload }))
    }
}

impl Encoder<Packet> for GamePacketCodec {
    type Error = Error;

    fn encode(&mut self, item: Packet, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let payload_len = item.payload.len();

        // CMSG_AUTH_SESSION is never encrypted - it uses 4 byte header
        // All other packets use 6 byte header (opcode + 2 zero bytes before encryption)
        if !self.header_crypt.is_initialized() {
            // Unencrypted: 2 bytes size (BE) + 2 bytes opcode (LE) + payload
            let total_size = payload_len + 2; // size includes opcode but not size field itself

            dst.reserve(4 + payload_len);

            // Size (BE) - includes opcode (2 bytes) + payload
            dst.put_u8((total_size >> 8) as u8);
            dst.put_u8((total_size & 0xFF) as u8);

            // Opcode (LE)
            dst.put_u16_le(item.opcode);

            // Payload
            dst.put_slice(&item.payload);
        } else {
            // Encrypted: 2 bytes size (BE) + 4 bytes opcode (LE + 2 zero bytes) + payload
            // Then encrypt the 6 byte header
            let total_size = payload_len + 4; // size includes opcode+zeros (4 bytes) but not size field itself

            dst.reserve(6 + payload_len);

            let mut header = [0u8; 6];
            header[0] = (total_size >> 8) as u8;
            header[1] = (total_size & 0xFF) as u8;

            let opcode_bytes = item.opcode.to_le_bytes();
            header[2] = opcode_bytes[0];
            header[3] = opcode_bytes[1];
            header[4] = 0;
            header[5] = 0;

            self.header_crypt.encrypt(&mut header);

            dst.put_slice(&header);
            dst.put_slice(&item.payload);
        }

        Ok(())
    }
}

/// A framed game server connection.
pub type GameConnection<S> = Framed<S, GamePacketCodec>;

/// Create a new game connection from a stream.
pub fn new_game_connection<S: AsyncRead + AsyncWrite>(stream: S) -> GameConnection<S> {
    Framed::new(stream, GamePacketCodec::new(GameHeaderCrypt::new()))
}
