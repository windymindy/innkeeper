//! Packet encoding and decoding traits.

use anyhow::{anyhow, Result};
use bytes::{Buf, Bytes, BytesMut};

/// Max length for short strings (player names, realm names, channel names).
pub const MAX_CSTRING_SHORT: usize = 256;

/// Max length for long strings (guild info, MOTD, chat messages).
pub const MAX_CSTRING_LONG: usize = 4096;

/// Read a null-terminated C string from any `Buf`, with a max length guard.
///
/// Returns `Err` if:
/// - The buffer is exhausted before a null terminator (unterminated string)
/// - The string exceeds `max_len` bytes (malformed/malicious packet)
pub fn read_cstring(buf: &mut impl Buf, max_len: usize) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        if !buf.has_remaining() {
            return Err(anyhow!("Unterminated C string"));
        }
        let b = buf.get_u8();
        if b == 0 {
            break;
        }
        if bytes.len() >= max_len {
            return Err(anyhow!("C string exceeds max length of {} bytes", max_len));
        }
        bytes.push(b);
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Read a packed GUID (variable length, 1–9 bytes) from any `Buf`.
///
/// WoW uses packed GUIDs to save bandwidth — a leading bitmask byte indicates
/// which of the 8 GUID bytes follow.  Only non-zero bytes are transmitted.
pub fn read_packed_guid(buf: &mut impl Buf) -> Result<u64> {
    if !buf.has_remaining() {
        return Err(anyhow!("read_packed_guid: buffer empty, need mask byte"));
    }
    let mask = buf.get_u8();
    let needed = mask.count_ones() as usize;
    if buf.remaining() < needed {
        return Err(anyhow!(
            "read_packed_guid: need {} GUID bytes for mask {:#04x}, have {}",
            needed,
            mask,
            buf.remaining()
        ));
    }
    let mut guid: u64 = 0;
    for i in 0..8 {
        if (mask & (1 << i)) != 0 {
            let byte = buf.get_u8();
            guid |= (byte as u64) << (i * 8);
        }
    }
    Ok(guid)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn read_cstring_returns_string_up_to_null_terminator() {
        let mut buf = Bytes::from_static(b"hello\0world");
        let result = read_cstring(&mut buf, MAX_CSTRING_SHORT).unwrap();
        assert_eq!(result, "hello");
        // Buffer should be advanced past the null byte, "world" remains
        assert_eq!(buf.len(), 5);
    }

    #[test]
    fn read_cstring_returns_empty_string_for_immediate_null() {
        let mut buf = Bytes::from_static(b"\0rest");
        let result = read_cstring(&mut buf, MAX_CSTRING_SHORT).unwrap();
        assert_eq!(result, "");
        assert_eq!(buf.len(), 4); // "rest" remains after consuming null byte
    }

    #[test]
    fn read_cstring_errors_on_unterminated_string() {
        let mut buf = Bytes::from_static(b"no null here");
        let err = read_cstring(&mut buf, MAX_CSTRING_LONG).unwrap_err();
        assert!(
            err.to_string().contains("Unterminated"),
            "Expected 'Unterminated' in error, got: {}",
            err
        );
    }

    #[test]
    fn read_cstring_errors_when_exceeding_max_len() {
        // 10 bytes of 'a' + null terminator, but max_len is 5
        let mut buf = Bytes::from_static(b"aaaaaaaaaa\0");
        let err = read_cstring(&mut buf, 5).unwrap_err();
        assert!(
            err.to_string().contains("exceeds max length"),
            "Expected 'exceeds max length' in error, got: {}",
            err
        );
    }

    #[test]
    fn read_cstring_succeeds_at_exact_max_len() {
        // Exactly 5 bytes + null terminator
        let mut buf = Bytes::from_static(b"abcde\0");
        let result = read_cstring(&mut buf, 5).unwrap();
        assert_eq!(result, "abcde");
    }

    #[test]
    fn read_cstring_errors_on_empty_buffer() {
        let mut buf = Bytes::new();
        let err = read_cstring(&mut buf, MAX_CSTRING_SHORT).unwrap_err();
        assert!(
            err.to_string().contains("Unterminated"),
            "Expected 'Unterminated' in error, got: {}",
            err
        );
    }

    #[test]
    fn read_cstring_works_with_byte_slice() {
        // Proves the generic works with &[u8] (used by realm handler)
        let data = b"realm\0rest";
        let mut buf: &[u8] = &data[..];
        let result = read_cstring(&mut buf, MAX_CSTRING_SHORT).unwrap();
        assert_eq!(result, "realm");
        assert_eq!(buf.len(), 4); // "rest" remains
    }

    #[test]
    fn read_cstring_replaces_invalid_utf8() {
        // 0xFF is invalid UTF-8 — from_utf8_lossy should replace it
        let mut buf = Bytes::from_static(b"hi\xFFthere\0");
        let result = read_cstring(&mut buf, MAX_CSTRING_SHORT).unwrap();
        assert!(result.contains("hi"));
        assert!(result.contains("there"));
        // The replacement character should be present
        assert!(result.contains('\u{FFFD}'));
    }
}
