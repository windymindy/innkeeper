//! Message formatting for display.
//!
//! Handles placeholder substitution in message format strings.
//! Supports placeholders: %time, %user, %message, %target, %channel, %rank

use chrono::Local;

/// Default format for WoW -> Discord messages.
pub const DEFAULT_WOW_TO_DISCORD_FORMAT: &str = "[%user]: %message";

/// Default format for Discord -> WoW messages.
pub const DEFAULT_DISCORD_TO_WOW_FORMAT: &str = "%user: %message";

/// Default format for guild notifications.
pub const DEFAULT_NOTIFICATION_FORMAT: &str = "%message";

/// Message formatter that substitutes placeholders in format strings.
#[derive(Debug, Clone)]
pub struct MessageFormatter {
    /// Format string for this formatter.
    format: String,
}

impl MessageFormatter {
    /// Create a new formatter with the given format string.
    pub fn new(format: impl Into<String>) -> Self {
        Self {
            format: format.into(),
        }
    }

    /// Create a formatter with the default WoW -> Discord format.
    pub fn wow_to_discord_default() -> Self {
        Self::new(DEFAULT_WOW_TO_DISCORD_FORMAT)
    }

    /// Create a formatter with the default Discord -> WoW format.
    pub fn discord_to_wow_default() -> Self {
        Self::new(DEFAULT_DISCORD_TO_WOW_FORMAT)
    }

    /// Format a message with the given context.
    ///
    /// Substitutes the following placeholders:
    /// - `%time` - Current time (HH:MM:SS)
    /// - `%user` - Username/sender
    /// - `%message` - The actual message content
    /// - `%target` - Target channel or player (for whispers)
    /// - `%channel` - Channel name
    /// - `%rank` - Rank name (for promotion/demotion events)
    pub fn format(&self, ctx: &FormatContext) -> String {
        self.format
            .replace("%time", &ctx.time())
            .replace("%user", &ctx.user)
            .replace("%message", &ctx.message)
            .replace("%target", &ctx.target)
            .replace("%channel", &ctx.channel)
            .replace("%rank", &ctx.rank)
            .replace("%achievement", &ctx.achievement)
    }

    /// Get the format string.
    pub fn format_string(&self) -> &str {
        &self.format
    }

    /// Calculate the maximum message length after formatting.
    ///
    /// Used to split messages that exceed WoW's 255 character limit.
    pub fn max_message_length(&self, user: &str, max_total: usize) -> usize {
        let overhead = self
            .format
            .replace("%time", &get_time())
            .replace("%user", user)
            .replace("%message", "")
            .replace("%target", "")
            .replace("%channel", "")
            .replace("%rank", "")
            .replace("%achievement", "")
            .len();

        max_total.saturating_sub(overhead)
    }
}

/// Context for message formatting.
#[derive(Debug, Clone, Default)]
pub struct FormatContext {
    /// The sender's name.
    pub user: String,
    /// The message content.
    pub message: String,
    /// Target player/channel (for whispers or channel messages).
    pub target: String,
    /// Channel name.
    pub channel: String,
    /// Rank name (for promotion/demotion events).
    pub rank: String,
    /// Achievement link/name (for achievement events).
    pub achievement: String,
}

impl FormatContext {
    /// Create a new format context.
    pub fn new(user: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            message: message.into(),
            target: String::new(),
            channel: String::new(),
            rank: String::new(),
            achievement: String::new(),
        }
    }

    /// Set the target.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    /// Set the channel.
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = channel.into();
        self
    }

    /// Set the rank.
    pub fn with_rank(mut self, rank: impl Into<String>) -> Self {
        self.rank = rank.into();
        self
    }

    /// Set the achievement.
    pub fn with_achievement(mut self, achievement: impl Into<String>) -> Self {
        self.achievement = achievement.into();
        self
    }

    /// Get the current time string.
    fn time(&self) -> String {
        get_time()
    }
}

/// Get the current time as HH:MM:SS string.
fn get_time() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

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

/// Split a message into chunks that fit within the max length (in bytes).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_format() {
        let formatter = MessageFormatter::new("[%user]: %message");
        let ctx = FormatContext::new("Player", "Hello world!");

        assert_eq!(formatter.format(&ctx), "[Player]: Hello world!");
    }

    #[test]
    fn test_format_with_target() {
        let formatter = MessageFormatter::new("[%user] whispers [%target]: %message");
        let ctx = FormatContext::new("Sender", "Hey there!").with_target("Receiver");

        assert_eq!(
            formatter.format(&ctx),
            "[Sender] whispers [Receiver]: Hey there!"
        );
    }

    #[test]
    fn test_format_with_time() {
        let formatter = MessageFormatter::new("[%time] %user: %message");
        let ctx = FormatContext::new("Player", "Test");
        let result = formatter.format(&ctx);

        // Time format should be HH:MM:SS
        assert!(result.contains("Player: Test"));
        assert!(result.starts_with('['));
    }

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("Hello world", 50);
        assert_eq!(chunks, vec!["Hello world"]);
    }

    #[test]
    fn test_split_message_on_space() {
        let chunks = split_message("Hello beautiful world", 15);
        // "Hello beautiful" is exactly 15 chars, so we split at "Hello" (last space within limit)
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
        // Must not panic; should split before 'é', not in the middle of it.
        assert_eq!(chunks[0], "caf");
        // "é rest" (6 bytes) splits again: "é r" fits in 4, space-split yields "é" + "rest"
        assert_eq!(chunks, vec!["caf", "é", "rest"]);
    }

    #[test]
    fn test_split_message_emoji() {
        // 🎉 is 4 bytes. "Hi 🎉 there" — max_len=4 lands inside the emoji.
        let chunks = split_message("Hi 🎉 there", 4);
        // Must not panic. "Hi" (space-split), then "🎉" (4 bytes, fits exactly), then remaining.
        assert_eq!(chunks, vec!["Hi", "🎉", "ther", "e"]);
    }

    #[test]
    fn test_split_message_all_multibyte() {
        // All 2-byte chars: "ééé" = 6 bytes. max_len=3 falls inside 2nd 'é'.
        let chunks = split_message("ééé", 3);
        // Should not panic. First chunk should be "é" (2 bytes, last boundary <= 3).
        assert_eq!(chunks[0], "é");
    }

    #[test]
    fn test_max_message_length() {
        let formatter = MessageFormatter::new("[%user]: %message");
        // Format overhead: "[Player]: " = 10 chars
        let max_len = formatter.max_message_length("Player", 255);
        assert_eq!(max_len, 245);
    }

    #[test]
    fn test_default_formats() {
        let wow_to_discord = MessageFormatter::wow_to_discord_default();
        let discord_to_wow = MessageFormatter::discord_to_wow_default();

        assert_eq!(
            wow_to_discord.format_string(),
            DEFAULT_WOW_TO_DISCORD_FORMAT
        );
        assert_eq!(
            discord_to_wow.format_string(),
            DEFAULT_DISCORD_TO_WOW_FORMAT
        );
    }
}
