//! Discord bot integration.
//!
//! This module provides the Discord bot functionality for bridging
//! messages between Discord and WoW.

pub mod client;
pub mod commands;
pub mod dashboard;
pub mod handler;
pub mod resolver;

// Re-export main types for external use
pub use client::{DiscordBotBuilder, DiscordChannels};
pub use commands::WowCommand;
