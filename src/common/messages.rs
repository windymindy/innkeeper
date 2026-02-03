//! Canonical message types for bridge communication.
//!
//! This module defines the single source of truth for message types
//! used in communication between Discord and WoW.

/// Bridge message for Discord <-> WoW communication.
///
/// Used for bidirectional message flow between Discord and the WoW game client.
#[derive(Debug, Clone)]
pub struct BridgeMessage {
    /// Sender's name (None for system messages from WoW, Some for player messages).
    pub sender: Option<String>,
    /// Message content.
    pub content: String,
    /// WoW chat type (see `protocol::game::chat::chat_events`).
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
    /// Optional custom format override (mainly used internally by bridge).
    pub format: Option<String>,
    /// Guild event type for filtering (e.g., "online", "offline", "joined", etc.)
    pub guild_event: Option<String>,
}

/// Message from Discord to be processed by the bridge.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    /// Sender's Discord display name.
    pub sender: String,
    /// Message content.
    pub content: String,
    /// Discord channel ID.
    pub channel_id: u64,
    /// Discord channel name.
    pub channel_name: String,
}

/// Command request from Discord.
#[derive(Debug, Clone)]
pub enum BridgeCommand {
    /// Request guild roster (online guildies).
    Who { reply_channel: u64 },
    /// Request guild MOTD.
    Gmotd { reply_channel: u64 },
}
