//! Game logic and message routing.
//!
//! This module contains:
//! - Message filtering with regex patterns
//! - Message formatting with placeholder substitution
//! - Channel routing between WoW and Discord
//! - The main bridge orchestrator

pub mod bridge;
pub mod client;
pub mod filter;
pub mod formatter;
pub mod router;

// Re-export commonly used types (from common module)
pub use crate::common::{
    BridgeChannels, BridgeCommand, DiscordMessage, OutgoingWowMessage, WowMessage,
};
pub use crate::discord::commands::CommandResponse;

// Re-export bridge-specific types
pub use bridge::Bridge;
pub use client::GameClient;
pub use filter::MessageFilter;
pub use formatter::{escape_discord_markdown, split_message, FormatContext, MessageFormatter};
pub use router::{Direction, MessageRouter, Route, SharedRouter, WowChannel};
