//! Canonical message types for bridge communication.
//!
//! This module defines the single source of truth for message types
//! used in communication between Discord and WoW.

use crate::common::types::{ChatMessage, GuildMember};
use crate::protocol::game::chat::chat_events;

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

impl BridgeMessage {
    /// Create a system message (no sender, system chat type).
    pub fn system(content: String) -> Self {
        Self {
            sender: None,
            content,
            chat_type: chat_events::CHAT_MSG_SYSTEM,
            channel_name: None,
            format: None,
            guild_event: None,
        }
    }

    /// Create a guild event message.
    pub fn guild_event(event: GuildEventInfo, content: String) -> Self {
        Self {
            sender: Some(event.player_name.clone()),
            content,
            chat_type: chat_events::CHAT_MSG_GUILD,
            channel_name: None,
            format: None,
            guild_event: Some(event),
        }
    }
}

impl From<ChatMessage> for BridgeMessage {
    fn from(msg: ChatMessage) -> Self {
        Self {
            sender: Some(msg.sender_name),
            content: msg.content,
            chat_type: msg.chat_type.to_id(),
            channel_name: msg.channel_name,
            format: msg.format,
            guild_event: None,
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
    /// Bot is disconnected from the realm.
    Disconnected,
    /// Update on guild statistics (online count).
    GuildStats { online_count: usize },
}

/// Data for the guild dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct GuildDashboardData {
    pub guild_name: String,
    pub realm: String,
    pub members: Vec<GuildMember>,
    pub online: bool,
}

/// Events for the dashboard renderer.
#[derive(Debug, Clone)]
pub enum DashboardEvent {
    /// Update dashboard with new data.
    Update(GuildDashboardData),
    /// Set dashboard status to offline (preserving last data).
    SetOffline,
}
