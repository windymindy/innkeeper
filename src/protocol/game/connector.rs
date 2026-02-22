//! Game server TCP connection and codec.

use bytes::{Buf, BufMut, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::protocol::game::header::GameHeaderCrypt;
use crate::protocol::packets::Packet;
use anyhow::Error;

/// Decoder state machine for partial reads.
///
/// Mirrors Scala's `var size = 0; var id = 0` stateful fields, but extended
/// to handle the case where a large-packet header is split between reads
/// (4 bytes arrived but the 5th hasn't).
///
/// ```text
/// Fresh → (decrypt 4 bytes, check 0x80)
///   ├─ normal packet → WaitingPayload { payload_size, opcode }
///   └─ large packet, 5th byte missing → NeedLargeExtra { header }
///
/// NeedLargeExtra → (decrypt 5th byte, parse full header)
///   └─ WaitingPayload { payload_size, opcode }
///
/// WaitingPayload → (payload arrives, emit packet)
///   └─ Fresh
/// ```
enum DecoderState {
    /// No header parsed yet.
    Fresh,
    /// 4 header bytes decrypted and consumed. Large-packet flag was set,
    /// but the 5th byte hasn't arrived yet. Stores the 4 decrypted bytes
    /// so we can resume without re-decrypting.
    NeedLargeExtra([u8; 4]),
    /// Header fully parsed and consumed. Waiting for payload bytes.
    WaitingPayload { payload_size: usize, opcode: u16 },
}

/// Codec for WoW game server packets.
///
/// Server-to-client header: 2-3 bytes size (BE) + 2 bytes opcode (LE).
/// Client-to-server header: 2 bytes size (BE) + 4 bytes opcode (LE + 2 zero bytes).
///
/// Header bytes are consumed and decrypted exactly once, then cached
/// across `decode()` calls if the payload hasn't fully arrived. This
/// prevents re-decrypting with a stateful cipher (RC4) on partial reads.
pub struct GamePacketCodec {
    header_crypt: GameHeaderCrypt,
    state: DecoderState,
}

impl GamePacketCodec {
    pub fn new(header_crypt: GameHeaderCrypt) -> Self {
        Self {
            header_crypt,
            state: DecoderState::Fresh,
        }
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
        // Try to advance the state machine until we have (payload_size, opcode).
        let (payload_size, opcode) = match self.state {
            DecoderState::WaitingPayload {
                payload_size,
                opcode,
            } => {
                // Header already parsed and consumed on a previous call.
                (payload_size, opcode)
            }

            DecoderState::NeedLargeExtra(header) => {
                // We previously decrypted 4 header bytes and found the large-packet
                // flag, but the 5th byte wasn't available. Try again.
                if src.is_empty() {
                    return Ok(None);
                }

                // Decrypt 5th byte separately.
                let mut extra = [src[0]];
                self.header_crypt.decrypt(&mut extra);
                src.advance(1);

                // Large header: 3-byte size + 2-byte opcode (split across header + extra)
                let size = (((header[0] & 0x7F) as usize) << 16)
                    | ((header[1] as usize) << 8)
                    | (header[2] as usize);
                let op = ((extra[0] as u16) << 8) | (header[3] as u16);
                (size - 2, op)
            }

            DecoderState::Fresh => {
                // Server to Client header is 2-3 bytes size + 2 bytes opcode.
                // Need at least 4 bytes for base header.
                if src.len() < 4 {
                    return Ok(None);
                }

                if self.header_crypt.is_initialized() {
                    // Encrypted path.
                    // Decrypt initial 4-byte header, consuming from buffer.
                    let mut header = [0u8; 4];
                    header.copy_from_slice(&src[..4]);
                    self.header_crypt.decrypt(&mut header);
                    src.advance(4);

                    // Check large packet flag AFTER decryption (size > 0x7FFF).
                    if (header[0] & 0x80) != 0 {
                        // Large packet: need 1 additional header byte.
                        if src.is_empty() {
                            // 5th byte hasn't arrived yet. Cache decrypted header
                            // so we can resume without re-decrypting.
                            self.state = DecoderState::NeedLargeExtra(header);
                            return Ok(None);
                        }

                        // Decrypt 5th byte separately.
                        let mut extra = [src[0]];
                        self.header_crypt.decrypt(&mut extra);
                        src.advance(1);

                        let size = (((header[0] & 0x7F) as usize) << 16)
                            | ((header[1] as usize) << 8)
                            | (header[2] as usize);
                        let op = ((extra[0] as u16) << 8) | (header[3] as u16);
                        (size - 2, op)
                    } else {
                        // Normal encrypted header: 2-byte size + 2-byte opcode
                        let size = ((header[0] as usize) << 8) | (header[1] as usize);
                        let op = u16::from_le_bytes([header[2], header[3]]);
                        (size - 2, op)
                    }
                } else {
                    // Unencrypted path (pre-auth, SMSG_AUTH_CHALLENGE).
                    let mut header = [0u8; 4];
                    header.copy_from_slice(&src[..4]);
                    src.advance(4);

                    let size = ((header[0] as usize) << 8) | (header[1] as usize);
                    let op = u16::from_le_bytes([header[2], header[3]]);
                    (size - 2, op)
                }
            }
        };

        // Check if full payload has arrived.
        if src.len() < payload_size {
            self.state = DecoderState::WaitingPayload {
                payload_size,
                opcode,
            };
            return Ok(None);
        }

        // Reset state and extract payload.
        self.state = DecoderState::Fresh;
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
