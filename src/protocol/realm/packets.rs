//! Realm server packet definitions.

/// Realm authentication result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthResult {
    Success,
    FailBanned,
    FailUnknownAccount,
    FailIncorrectPassword,
    FailAlreadyOnline,
    FailNoTime,
    FailDbBusy,
    FailVersionInvalid,
    FailVersionUpdate,
    FailSuspended,
    FailTrialEnded,
    Unknown(u8),
}

impl AuthResult {
    pub fn from_code(code: u8) -> Self {
        match code {
            0x00 => Self::Success,
            0x03 => Self::FailBanned,
            0x04 => Self::FailUnknownAccount,
            0x05 => Self::FailIncorrectPassword,
            0x06 => Self::FailAlreadyOnline,
            0x07 => Self::FailNoTime,
            0x08 => Self::FailDbBusy,
            0x09 => Self::FailVersionInvalid,
            0x0A => Self::FailVersionUpdate,
            0x0C => Self::FailSuspended,
            0x0E => Self::FailTrialEnded,
            other => Self::Unknown(other),
        }
    }
}

/// Information about a realm server.
#[derive(Debug, Clone)]
pub struct RealmInfo {
    /// Realm ID.
    pub id: u8,
    /// Realm name.
    pub name: String,
    /// Realm address (host:port).
    pub address: String,
    /// Realm type (PvP, PvE, etc.).
    pub _realm_type: u8,
    /// Realm flags (offline, recommended, etc.).
    pub _flags: u8,
    /// Number of characters the account has on this realm.
    pub _characters: u8,
}

impl RealmInfo {
    /// Parse the address into host and port.
    pub fn parse_address(&self) -> Option<(&str, u16)> {
        let parts: Vec<&str> = self.address.split(':').collect();
        if parts.len() == 2 {
            let port = parts[1].parse().ok()?;
            Some((parts[0], port))
        } else {
            None
        }
    }
}

/// Realm flag constants (for internal use).
pub mod realm_flags {
    pub const NONE: u8 = 0x00;
    pub const INVALID: u8 = 0x01;
    pub const OFFLINE: u8 = 0x02;
}
