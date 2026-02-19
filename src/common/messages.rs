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
    /// Error response (e.g., game disconnected).
    Error(String),
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

// ---------------------------------------------------------------------------
// Text splitting utilities
// ---------------------------------------------------------------------------

/// Find the last UTF-8 char boundary at or before `byte_index` in `s`.
///
/// Returns a byte offset that is safe to use for slicing `s`.
fn floor_char_boundary(s: &str, byte_index: usize) -> usize {
    if byte_index >= s.len() {
        return s.len();
    }
    // Walk backward until we find a char boundary
    let mut i = byte_index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Split a message into chunks that fit within `max_len` bytes.
///
/// Tries to split on word boundaries when possible. Never splits in the
/// middle of a multi-byte UTF-8 character.
pub fn split_message(message: &str, max_len: usize) -> Vec<String> {
    if message.len() <= max_len {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        // Skip leading spaces left over from previous word-boundary splits
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let split_at = floor_char_boundary(remaining, max_len);

        // If max_len is smaller than the first character, force at least one
        // character to avoid an infinite loop.
        if split_at == 0 {
            let first_char_end = remaining
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            chunks.push(remaining[..first_char_end].to_string());
            remaining = &remaining[first_char_end..];
            continue;
        }

        let chunk = &remaining[..split_at];

        // Try to find a space to split on
        if let Some(space_idx) = chunk.rfind(' ') {
            chunks.push(remaining[..space_idx].to_string());
            remaining = &remaining[space_idx + 1..];
        } else {
            // No space found, hard split at char boundary
            chunks.push(chunk.to_string());
            remaining = &remaining[split_at..];
        }
    }

    chunks
}

/// Split a message into chunks, preserving newline structure.
///
/// First groups lines into chunks that fit under `max_len`. If any single
/// line exceeds `max_len`, it delegates to [`split_message`] for UTF-8-safe
/// word-boundary splitting. Newlines between lines within a chunk are preserved.
pub fn split_message_preserving_newlines(message: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for line in message.lines() {
        if line.len() > max_len {
            // Flush current chunk first
            if !current_chunk.is_empty() {
                chunks.push(std::mem::take(&mut current_chunk));
            }
            // Delegate to UTF-8-safe splitter for the oversized line
            chunks.extend(split_message(line, max_len));
        } else if current_chunk.len() + line.len() + 1 > max_len {
            // Adding this line would exceed limit, flush current chunk
            chunks.push(std::mem::take(&mut current_chunk));
            current_chunk = line.to_string();
        } else {
            // Add line to current chunk
            if !current_chunk.is_empty() {
                current_chunk.push('\n');
            }
            current_chunk.push_str(line);
        }
    }

    // Don't forget the last chunk
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- split_message tests ---

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("Hello world", 50);
        assert_eq!(chunks, vec!["Hello world"]);
    }

    #[test]
    fn test_split_message_on_space() {
        let chunks = split_message("Hello beautiful world", 15);
        assert_eq!(chunks, vec!["Hello", "beautiful world"]);
    }

    #[test]
    fn test_split_message_no_space() {
        let chunks = split_message("HelloBeautifulWorld", 10);
        assert_eq!(chunks, vec!["HelloBeaut", "ifulWorld"]);
    }

    #[test]
    fn test_split_message_multibyte_utf8() {
        // "café" is 5 bytes: c(1) a(1) f(1) é(2). max_len=4 lands inside 'é'.
        let chunks = split_message("café rest", 4);
        assert_eq!(chunks[0], "caf");
        assert_eq!(chunks, vec!["caf", "é", "rest"]);
    }

    #[test]
    fn test_split_message_emoji() {
        // 🎉 is 4 bytes. "Hi 🎉 there" — max_len=4 lands inside the emoji.
        let chunks = split_message("Hi 🎉 there", 4);
        assert_eq!(chunks, vec!["Hi", "🎉", "ther", "e"]);
    }

    #[test]
    fn test_split_message_all_multibyte() {
        // All 2-byte chars: "ééé" = 6 bytes. max_len=3 falls inside 2nd 'é'.
        let chunks = split_message("ééé", 3);
        assert_eq!(chunks[0], "é");
    }

    // --- split_message_preserving_newlines tests ---

    #[test]
    fn test_newline_split_short() {
        let chunks = split_message_preserving_newlines("Hello\nworld", 50);
        assert_eq!(chunks, vec!["Hello\nworld"]);
    }

    #[test]
    fn test_newline_split_at_boundary() {
        let chunks = split_message_preserving_newlines("Hello\nworld\nfoo", 11);
        // "Hello\nworld" = 11 bytes, fits exactly
        assert_eq!(chunks, vec!["Hello\nworld", "foo"]);
    }

    #[test]
    fn test_newline_split_long_line_utf8() {
        // A single line with multibyte chars that exceeds max_len
        let chunks = split_message_preserving_newlines("café résumé", 6);
        // Should NOT produce U+FFFD replacement characters
        for chunk in &chunks {
            assert!(
                !chunk.contains('\u{FFFD}'),
                "chunk contained replacement char: {chunk:?}"
            );
        }
    }

    #[test]
    fn test_newline_split_mixed() {
        let msg = "short\nAnExtremelyLongLineWithNoSpaces\nend";
        let chunks = split_message_preserving_newlines(msg, 15);
        assert_eq!(chunks[0], "short");
        // The long line should be split via split_message, never corrupted
        assert!(chunks.iter().all(|c| c.len() <= 15));
    }
}
