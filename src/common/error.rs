//! Error types for the application.

use thiserror::Error;

/// Top-level application error.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Discord error: {0}")]
    Discord(#[from] DiscordError),

    #[error("Connection error: {0}")]
    Connection(#[from] ConnectionError),
}

/// Configuration-related errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    IoError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse config: {message}")]
    #[allow(dead_code)]
    ParseError { message: String },

    #[error("Config validation failed: {message}")]
    ValidationError { message: String },

    #[error("Missing required field: {field}")]
    #[allow(dead_code)]
    MissingField { field: String },

    #[error("Invalid value for '{field}': {message}")]
    #[allow(dead_code)]
    InvalidValue { field: String, message: String },
}

/// Protocol-related errors (WoW communication).
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Invalid packet: {message}")]
    InvalidPacket { message: String },

    #[error("Unexpected opcode: expected {expected}, got {actual}")]
    UnexpectedOpcode { expected: u16, actual: u16 },

    #[error("Packet too short: need {needed} bytes, got {got}")]
    PacketTooShort { needed: usize, got: usize },

    #[error("Invalid string encoding: {message}")]
    InvalidString { message: String },

    #[error("Authentication failed: {reason}")]
    AuthFailed { reason: String },

    #[error("Character not found: {name}")]
    #[allow(dead_code)]
    CharacterNotFound { name: String },

    #[error("Realm not found: {name}")]
    #[allow(dead_code)]
    RealmNotFound { name: String },

    #[error("Encryption error: {message}")]
    EncryptionError { message: String },

    #[error("Decryption error: {message}")]
    #[allow(dead_code)]
    DecryptionError { message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Connection-related errors.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("Failed to connect to {host}:{port}: {source}")]
    ConnectFailed {
        host: String,
        port: u16,
        #[source]
        source: std::io::Error,
    },

    #[error("Connection closed by remote")]
    ConnectionClosed,

    #[error("Connection timeout")]
    #[allow(dead_code)]
    Timeout,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Maximum reconnection attempts exceeded")]
    #[allow(dead_code)]
    MaxReconnectAttempts,
}

/// Discord-related errors.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum DiscordError {
    #[error("Failed to connect to Discord: {message}")]
    ConnectionFailed { message: String },

    #[error("Failed to send message: {message}")]
    SendFailed { message: String },

    #[error("Channel not found: {channel_id}")]
    ChannelNotFound { channel_id: u64 },

    #[error("Permission denied: {message}")]
    PermissionDenied { message: String },

    #[error("Rate limited")]
    RateLimited,

    #[error("Serenity error: {0}")]
    Serenity(#[from] serenity::Error),
}

/// Result type alias using AppError.
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, AppError>;

/// Result type alias for protocol operations.
#[allow(dead_code)]
pub type ProtocolResult<T> = std::result::Result<T, ProtocolError>;

/// Result type alias for connection operations.
#[allow(dead_code)]
pub type ConnectionResult<T> = std::result::Result<T, ConnectionError>;

/// Result type alias for Discord operations.
#[allow(dead_code)]
pub type DiscordResult<T> = std::result::Result<T, DiscordError>;
