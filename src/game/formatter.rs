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
