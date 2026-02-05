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
    FailInvalidServer,
    FailSuspended,
    FailNoAccess,
    FailSurveySuccess,
    FailParentControl,
    FailTrialEnded,
    FailNewDevice,
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
            0x0B => Self::FailInvalidServer,
            0x0C => Self::FailSuspended,
            0x0D => Self::FailNoAccess,
            0x0E => Self::FailSurveySuccess,
            0x0F => Self::FailParentControl,
            0x11 => Self::FailTrialEnded,
            0x17 => Self::FailNewDevice,
            other => Self::Unknown(other),
        }
    }

    /// Returns true if the authentication was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success | Self::FailSurveySuccess)
    }

    /// Returns a human-readable error message for this auth result.
    pub fn get_message(&self) -> String {
        match self {
            Self::Success => "Success!".to_string(),
            Self::FailSurveySuccess => "Success!".to_string(),
            Self::FailBanned => "Your account has been banned!".to_string(),
            Self::FailIncorrectPassword => "Incorrect username or password!".to_string(),
            Self::FailUnknownAccount => "Incorrect username or password!".to_string(),
            Self::FailAlreadyOnline => {
                "Your account is already online. Wait a moment and try again!".to_string()
            }
            Self::FailVersionInvalid | Self::FailVersionUpdate => {
                "Invalid game version for this server!".to_string()
            }
            Self::FailSuspended => "Your account has been suspended!".to_string(),
            Self::FailNoAccess => {
                "Login failed! You do not have access to this server!".to_string()
            }
            Self::FailNoTime => "Account has no game time!".to_string(),
            Self::FailDbBusy => "Database is busy. Try again later!".to_string(),
            Self::FailInvalidServer => "Invalid server selected!".to_string(),
            Self::FailParentControl => "Account is restricted by parental controls!".to_string(),
            Self::FailTrialEnded => "Trial period has ended!".to_string(),
            Self::FailNewDevice => {
                "Approve new device login! Please check your email inbox for a message from Ascension and click the verification link to complete the process.".to_string()
            },
            Self::Unknown(code) => {
                format!("Failed to login to realm server! Error code: {:02X}", code)
            }
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
