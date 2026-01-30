//! Common utilities and types shared across the application.

pub mod error;
pub mod messages;
pub mod reconnect;
pub mod resources;
pub mod types;

// Re-export commonly used message types
pub use messages::{
    BridgeChannels, BridgeCommand, DiscordMessage, IncomingWowMessage, OutgoingWowMessage,
    WowMessage,
};
