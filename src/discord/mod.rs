//! Discord bot integration.
//!
//! This module provides the Discord bot functionality for bridging
//! messages between Discord and WoW.

pub mod bot;
pub mod commands;
pub mod handler;
pub mod resolver;

// Re-export main types
pub use bot::{
    run_discord_bot, ChannelMappingConfig, ChatDirection, DiscordBotConfig, DiscordReceiver,
    DiscordToWowMessage, WowSender, WowToDiscordMessage,
};

pub use commands::{
    format_gmotd_response, format_who_response, format_who_search_response, CommandHandler,
    CommandResponse, WowCommand,
};

pub use handler::{
    create_bridge_channels, BridgeHandler, BridgeState, ChannelConfig, IncomingWowMessage,
    OutgoingWowMessage,
};

pub use resolver::{split_message, MessageResolver};
