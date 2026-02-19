//! Common utilities and types shared across the application.

pub mod messages;
pub mod resources;
pub mod types;

// Re-export message types from messages module
pub use messages::{
    BridgeCommand, BridgeMessage, CommandResponseData, DiscordMessage, GuildEventInfo,
};

// Re-export text utilities
pub use messages::{split_message, split_message_preserving_newlines};

// Re-export status types
pub use messages::ActivityStatus;

// Re-export achievement functions from resources
pub use resources::{get_achievement_name, get_achievements};
