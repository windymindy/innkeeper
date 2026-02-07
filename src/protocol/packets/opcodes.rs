//! WoW protocol opcodes for WotLK/Ascension.

// ============================================================================
// Realm Server Opcodes
// ============================================================================

/// Realm authentication opcodes.
pub mod realm {
    /// Client -> Server: Initial login challenge
    pub const AUTH_LOGON_CHALLENGE: u8 = 0x00;
    /// Server -> Client: Challenge response
    pub const AUTH_LOGON_PROOF: u8 = 0x01;
    /// Client -> Server: Reconnect challenge
    pub const AUTH_RECONNECT_CHALLENGE: u8 = 0x02;
    /// Server -> Client: Reconnect proof
    pub const AUTH_RECONNECT_PROOF: u8 = 0x03;
    /// Client -> Server: Request realm list
    pub const REALM_LIST: u8 = 0x10;
}

/// Authentication response codes from game server.
#[allow(dead_code)]
pub mod auth_response {
    pub const AUTH_OK: u8 = 0x0C;
    pub const AUTH_FAILED: u8 = 0x0D;
    pub const AUTH_REJECT: u8 = 0x0E;
    pub const AUTH_BAD_SERVER_PROOF: u8 = 0x0F;
    pub const AUTH_UNAVAILABLE: u8 = 0x10;
    pub const AUTH_SYSTEM_ERROR: u8 = 0x11;
    pub const AUTH_BILLING_ERROR: u8 = 0x12;
    pub const AUTH_BILLING_EXPIRED: u8 = 0x13;
    pub const AUTH_VERSION_MISMATCH: u8 = 0x14;
    pub const AUTH_UNKNOWN_ACCOUNT: u8 = 0x15;
    pub const AUTH_INCORRECT_PASSWORD: u8 = 0x16;
    pub const AUTH_SESSION_EXPIRED: u8 = 0x17;
    pub const AUTH_SERVER_SHUTTING_DOWN: u8 = 0x18;
    pub const AUTH_ALREADY_LOGGING_IN: u8 = 0x19;
    pub const AUTH_LOGIN_SERVER_NOT_FOUND: u8 = 0x1A;
    pub const AUTH_WAIT_QUEUE: u8 = 0x1B;
    pub const AUTH_BANNED: u8 = 0x1C;
    pub const AUTH_ALREADY_ONLINE: u8 = 0x1D;
    pub const AUTH_NO_TIME: u8 = 0x1E;
    pub const AUTH_DB_BUSY: u8 = 0x1F;
    pub const AUTH_SUSPENDED: u8 = 0x20;
    pub const AUTH_PARENTAL_CONTROL: u8 = 0x21;
    pub const AUTH_LOCKED_ENFORCED: u8 = 0x22;

    /// Get a human-readable name for an auth response code.
    pub fn code_name(code: u8) -> &'static str {
        match code {
            AUTH_OK => "AUTH_OK",
            AUTH_FAILED => "AUTH_FAILED",
            AUTH_REJECT => "AUTH_REJECT",
            AUTH_BAD_SERVER_PROOF => "AUTH_BAD_SERVER_PROOF",
            AUTH_UNAVAILABLE => "AUTH_UNAVAILABLE",
            AUTH_SYSTEM_ERROR => "AUTH_SYSTEM_ERROR",
            AUTH_BILLING_ERROR => "AUTH_BILLING_ERROR",
            AUTH_BILLING_EXPIRED => "AUTH_BILLING_EXPIRED",
            AUTH_VERSION_MISMATCH => "AUTH_VERSION_MISMATCH",
            AUTH_UNKNOWN_ACCOUNT => "AUTH_UNKNOWN_ACCOUNT",
            AUTH_INCORRECT_PASSWORD => "AUTH_INCORRECT_PASSWORD",
            AUTH_SESSION_EXPIRED => "AUTH_SESSION_EXPIRED",
            AUTH_SERVER_SHUTTING_DOWN => "AUTH_SERVER_SHUTTING_DOWN",
            AUTH_ALREADY_LOGGING_IN => "AUTH_ALREADY_LOGGING_IN",
            AUTH_LOGIN_SERVER_NOT_FOUND => "AUTH_LOGIN_SERVER_NOT_FOUND",
            AUTH_WAIT_QUEUE => "AUTH_WAIT_QUEUE",
            AUTH_BANNED => "AUTH_BANNED",
            AUTH_ALREADY_ONLINE => "AUTH_ALREADY_ONLINE",
            AUTH_NO_TIME => "AUTH_NO_TIME",
            AUTH_DB_BUSY => "AUTH_DB_BUSY",
            AUTH_SUSPENDED => "AUTH_SUSPENDED",
            AUTH_PARENTAL_CONTROL => "AUTH_PARENTAL_CONTROL",
            AUTH_LOCKED_ENFORCED => "AUTH_LOCKED_ENFORCED",
            _ => "UNKNOWN",
        }
    }

    /// Get a user-friendly error message for an auth response code.
    pub fn error_message(code: u8) -> &'static str {
        match code {
            AUTH_OK => "Success!",
            AUTH_UNKNOWN_ACCOUNT | AUTH_INCORRECT_PASSWORD => "Incorrect username or password!",
            AUTH_VERSION_MISMATCH => "Invalid game version for this server!",
            AUTH_BANNED => "Your account has been banned!",
            AUTH_ALREADY_LOGGING_IN | AUTH_ALREADY_ONLINE => "Your account is already online! Log it off or wait a minute if already logging off.",
            AUTH_SUSPENDED => "Your account has been suspended!",
            _ => "Failed to login to game server!",
        }
    }
}

// ============================================================================
// Game Server Opcodes (WotLK 3.3.5a)
// ============================================================================

// --- Authentication ---
pub const SMSG_AUTH_CHALLENGE: u16 = 0x01EC;
pub const CMSG_AUTH_SESSION: u16 = 0x01ED;
pub const SMSG_AUTH_RESPONSE: u16 = 0x01EE;

// --- Character ---
pub const CMSG_CHAR_ENUM: u16 = 0x0037;
pub const SMSG_CHAR_ENUM: u16 = 0x003B;
pub const CMSG_PLAYER_LOGIN: u16 = 0x003D;

// --- World ---
pub const SMSG_LOGIN_VERIFY_WORLD: u16 = 0x0236;
pub const CMSG_LOGOUT_REQUEST: u16 = 0x004B;
pub const SMSG_LOGOUT_COMPLETE: u16 = 0x004D;

// --- Keep-alive ---
pub const CMSG_PING: u16 = 0x01DC;
pub const SMSG_PONG: u16 = 0x01DD;
pub const CMSG_KEEP_ALIVE: u16 = 0x0407;

// --- Time ---
pub const SMSG_TIME_SYNC_REQ: u16 = 0x0390;
pub const CMSG_TIME_SYNC_RESP: u16 = 0x0391;

// --- Chat ---
pub const SMSG_MESSAGECHAT: u16 = 0x0096;
pub const CMSG_MESSAGECHAT: u16 = 0x0095;
pub const SMSG_GM_MESSAGECHAT: u16 = 0x03B3;

// --- Channels ---
pub const CMSG_JOIN_CHANNEL: u16 = 0x0097;
pub const CMSG_LEAVE_CHANNEL: u16 = 0x0098;
pub const SMSG_CHANNEL_NOTIFY: u16 = 0x0099;
pub const SMSG_CHANNEL_LIST: u16 = 0x009B;

pub const SMSG_CHAT_PLAYER_NOT_FOUND: u16 = 0x02A9;

// --- Guild ---
pub const CMSG_GUILD_QUERY: u16 = 0x0054;
pub const SMSG_GUILD_QUERY: u16 = 0x0055;
pub const CMSG_GUILD_ROSTER: u16 = 0x0089;
pub const SMSG_GUILD_ROSTER: u16 = 0x008A;
pub const SMSG_GUILD_EVENT: u16 = 0x0092;

// --- Name queries ---
pub const CMSG_NAME_QUERY: u16 = 0x0050;
pub const SMSG_NAME_QUERY: u16 = 0x0051;

// --- Server messages ---
pub const SMSG_SERVER_MESSAGE: u16 = 0x0291;
pub const SMSG_NOTIFICATION: u16 = 0x01CB;
pub const SMSG_MOTD: u16 = 0x033D;

// --- Warden (not used on Ascension) ---
pub const SMSG_WARDEN_DATA: u16 = 0x02E6;
pub const CMSG_WARDEN_DATA: u16 = 0x02E7;

// --- Miscellaneous ---
pub const SMSG_INVALIDATE_PLAYER: u16 = 0x031C;
pub const SMSG_INIT_WORLD_STATES: u16 = 0x02C2;
pub const SMSG_UPDATE_OBJECT: u16 = 0x00A9;
pub const SMSG_COMPRESSED_UPDATE_OBJECT: u16 = 0x01F6;

pub const CMSG_GAMEOBJ_USE: u16 = 0x00B1;

/// Get a human-readable name for an opcode.
pub fn opcode_name(opcode: u16) -> &'static str {
    match opcode {
        SMSG_AUTH_CHALLENGE => "SMSG_AUTH_CHALLENGE",
        CMSG_AUTH_SESSION => "CMSG_AUTH_SESSION",
        SMSG_AUTH_RESPONSE => "SMSG_AUTH_RESPONSE",
        CMSG_CHAR_ENUM => "CMSG_CHAR_ENUM",
        SMSG_CHAR_ENUM => "SMSG_CHAR_ENUM",
        CMSG_PLAYER_LOGIN => "CMSG_PLAYER_LOGIN",
        SMSG_LOGIN_VERIFY_WORLD => "SMSG_LOGIN_VERIFY_WORLD",
        CMSG_LOGOUT_REQUEST => "CMSG_LOGOUT_REQUEST",
        CMSG_PING => "CMSG_PING",
        SMSG_PONG => "SMSG_PONG",
        SMSG_TIME_SYNC_REQ => "SMSG_TIME_SYNC_REQ",
        CMSG_TIME_SYNC_RESP => "CMSG_TIME_SYNC_RESP",
        SMSG_MESSAGECHAT => "SMSG_MESSAGECHAT",
        CMSG_MESSAGECHAT => "CMSG_MESSAGECHAT",
        CMSG_JOIN_CHANNEL => "CMSG_JOIN_CHANNEL",
        SMSG_CHANNEL_NOTIFY => "SMSG_CHANNEL_NOTIFY",
        SMSG_CHAT_PLAYER_NOT_FOUND => "SMSG_CHAT_PLAYER_NOT_FOUND",
        CMSG_GUILD_QUERY => "CMSG_GUILD_QUERY",
        SMSG_GUILD_QUERY => "SMSG_GUILD_QUERY",
        CMSG_GUILD_ROSTER => "CMSG_GUILD_ROSTER",
        SMSG_GUILD_ROSTER => "SMSG_GUILD_ROSTER",
        SMSG_GUILD_EVENT => "SMSG_GUILD_EVENT",
        CMSG_NAME_QUERY => "CMSG_NAME_QUERY",
        SMSG_NAME_QUERY => "SMSG_NAME_QUERY",
        SMSG_WARDEN_DATA => "SMSG_WARDEN_DATA",
        CMSG_WARDEN_DATA => "CMSG_WARDEN_DATA",
        SMSG_NOTIFICATION => "SMSG_NOTIFICATION",
        SMSG_MOTD => "SMSG_MOTD",
        SMSG_GM_MESSAGECHAT => "SMSG_GM_MESSAGECHAT",
        SMSG_SERVER_MESSAGE => "SMSG_SERVER_MESSAGE",
        SMSG_INIT_WORLD_STATES => "SMSG_INIT_WORLD_STATES",
        CMSG_GAMEOBJ_USE => "CMSG_GAMEOBJ_USE",
        _ => "UNKNOWN",
    }
}
