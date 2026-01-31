//! Canonical message types for bridge communication.
//!
//! This module defines the single source of truth for message types
//! used in communication between Discord and WoW.

/// Message from WoW destined for Discord.
///
/// Used when forwarding chat messages from the WoW game client to Discord channels.
#[derive(Debug, Clone)]
pub struct IncomingWowMessage {
    /// Sender's name (None for system messages).
    pub sender: Option<String>,
    /// Message content.
    pub content: String,
    /// WoW chat type (see `protocol::game::chat::chat_events`).
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
}

/// Message from Discord destined for WoW.
///
/// Used when forwarding messages from Discord to the WoW game client.
#[derive(Debug, Clone)]
pub struct OutgoingWowMessage {
    /// WoW chat type to send as.
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
    /// Sender's Discord display name.
    pub sender: String,
    /// Message content (already formatted).
    pub content: String,
}

/// Message from WoW with optional format override.
///
/// Extended version of IncomingWowMessage used internally by the bridge.
#[derive(Debug, Clone)]
pub struct WowMessage {
    /// Sender's name (None for system messages).
    pub sender: Option<String>,
    /// Message content.
    pub content: String,
    /// WoW chat type.
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
    /// Custom format override (optional).
    pub format: Option<String>,
}

impl From<WowMessage> for IncomingWowMessage {
    fn from(msg: WowMessage) -> Self {
        Self {
            sender: msg.sender,
            content: msg.content,
            chat_type: msg.chat_type,
            channel_name: msg.channel_name,
        }
    }
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
