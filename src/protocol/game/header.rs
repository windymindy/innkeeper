//! Game packet header handling for WotLK/Ascension.

/// Header crypt for game packets.
/// On Ascension/WotLK variant in wowchat, this is effectively a NOP.
#[derive(Debug, Default)]
pub struct GameHeaderCrypt {
    initialized: bool,
}

impl GameHeaderCrypt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize the crypt with the session key.
    pub fn init(&mut self, _key: &[u8]) {
        self.initialized = true;
    }

    /// Decrypt the header data (NOP for Ascension).
    pub fn decrypt(&self, _data: &mut [u8]) {
        // Ascension doesn't encrypt game headers in this implementation
    }

    /// Encrypt the header data (NOP for Ascension).
    pub fn encrypt(&self, _data: &mut [u8]) {
        // Ascension doesn't encrypt game headers in this implementation
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}
