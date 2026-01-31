//! Common utilities and types shared across the application.

pub mod messages;
pub mod reconnect;
pub mod resources;
pub mod types;

// Re-export message types from messages module
pub use messages::{
    BridgeCommand, DiscordMessage, IncomingWowMessage, OutgoingWowMessage, WowMessage,
};

// BridgeChannels is now in the bridge module
pub use crate::bridge::BridgeChannels;
