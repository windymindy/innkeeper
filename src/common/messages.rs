//! Canonical message types for bridge communication.
//!
//! This module defines the single source of truth for message types
//! used in communication between Discord and WoW.

use crate::common::types::GuildMember;

/// Guild event data extracted from SMSG_GUILD_EVENT.
#[derive(Debug, Clone)]
pub struct GuildEventInfo {
    /// The event type name (e.g., "promoted", "demoted", "online").
    pub event_name: String,
    /// The player who triggered the event (e.g., the one who promoted/demoted).
    pub player_name: String,
    /// The target player (for promotion/demotion/removal events).
    pub target_name: Option<String>,
    /// The rank name (for promotion/demotion events).
    pub rank_name: Option<String>,
    /// The achievement ID (for achievement events).
    pub achievement_id: Option<u32>,
}

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
    /// Guild event information for filtering and formatting.
    pub guild_event: Option<GuildEventInfo>,
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
    Who {
        args: Option<String>,
        reply_channel: u64,
    },
    /// Request guild MOTD.
    Gmotd { reply_channel: u64 },
}

/// Structured response data for Discord commands.
#[derive(Debug, Clone)]
pub enum CommandResponseData {
    /// Simple text response.
    String(String),
    /// List of guild members (!who).
    WhoList(Vec<GuildMember>, Option<String>), // (members, guild_name)
    /// Single member search result (!who <name>).
    WhoSearch(String, Option<GuildMember>, Option<String>), // (search_input, member, guild_name)
    /// Guild MOTD (!gmotd).
    GuildMotd(Option<String>, Option<String>), // (motd, guild_name)
}

/// Represents a change in the bot's activity status.
#[derive(Debug, Clone, PartialEq)]
pub enum ActivityStatus {
    /// Bot is connecting to the game server.
    Connecting,
    /// Bot is connected to a realm.
    ConnectedToRealm(String),
    /// Update on guild statistics (online count).
    GuildStats { online_count: usize },
}
