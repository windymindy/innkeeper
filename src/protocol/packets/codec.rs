//! Packet encoding and decoding traits.

use bytes::{Bytes, BytesMut};

/// A WoW protocol packet.
#[derive(Debug, Clone)]
pub struct Packet {
    pub opcode: u16,
    pub payload: Bytes,
}

impl Packet {
    /// Create a new packet with the given opcode and payload.
    pub fn new(opcode: u16, payload: impl Into<Bytes>) -> Self {
        Self {
            opcode,
            payload: payload.into(),
        }
    }

    /// Create an empty packet with just an opcode.
    pub fn empty(opcode: u16) -> Self {
        Self {
            opcode,
            payload: Bytes::new(),
        }
    }
}

/// Trait for types that can be encoded into packet payload.
pub trait PacketEncode {
    fn encode(&self, buf: &mut BytesMut);
}

/// Trait for types that can be decoded from packet payload.
pub trait PacketDecode: Sized {
    type Error;
    fn decode(buf: &mut Bytes) -> Result<Self, Self::Error>;
}
