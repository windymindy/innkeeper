//! Emoji, mention, and link resolution.
//!
//! Handles translation between WoW item links and Discord-friendly formats,
//! emoji resolution, and @mention handling.

use regex::Regex;
use serenity::cache::Cache;
use std::sync::OnceLock;

/// WotLK/Ascension link site for item/spell lookups.
const LINK_SITE: &str = "https://db.ascension.gg/";

/// Message resolver for WoW <-> Discord message translation.
#[derive(Debug, Clone)]
pub struct MessageResolver {
    /// Compiled regex patterns for link resolution.
    link_patterns: Vec<(&'static str, Regex)>,
    /// Pattern for color coding.
    color_pattern: Regex,
    /// Pattern for color ending.
    color_end_pattern: Regex,
    /// Pattern for texture coding.
    texture_pattern: Regex,
}

impl Default for MessageResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageResolver {
    /// Create a new message resolver with WotLK/Ascension patterns.
    pub fn new() -> Self {
        Self {
            link_patterns: vec![
                (
                    "item",
                    Regex::new(r"\|.+?\|Hitem:(\d+):.+?\|h\[(.+?)\]\|h\|r\s?").unwrap(),
                ),
                (
                    "spell",
                    Regex::new(
                        r"\|.+?\|(?:Hspell|Henchant|Htalent)?:(\d+).*?\|h\[(.+?)\]\|h\|r\s?",
                    )
                    .unwrap(),
                ),
                (
                    "quest",
                    Regex::new(r"\|.+?\|Hquest:(\d+):.+?\|h\[(.+?)\]\|h\|r\s?").unwrap(),
                ),
                (
                    "achievement",
                    Regex::new(r"\|.+?\|Hachievement:(\d+):.+?\|h\[(.+?)\]\|h\|r\s?").unwrap(),
                ),
                (
                    "spell",
                    Regex::new(r"\|Htrade:(\d+):.+?\|h\[(.+?)\]\|h\s?").unwrap(),
                ),
            ],
            color_pattern: Regex::new(r"\|c[0-9a-fA-F]{8}(.*?)\|r").unwrap(),
            color_end_pattern: Regex::new(r"\|c[0-9a-fA-F]{8}").unwrap(),
            texture_pattern: Regex::new(r"\|T(.*?)\|t").unwrap(),
        }
    }

    /// Resolve WoW item/spell/quest links to Discord-friendly markdown.
    pub fn resolve_links(&self, message: &str) -> String {
        let mut result = message.to_string();

        for (link_type, pattern) in &self.link_patterns {
            result = pattern
                .replace_all(&result, |caps: &regex::Captures| {
                    let id = &caps[1];
                    let name = &caps[2];
                    format!("[{}] (<{}?{}={}>) ", name, LINK_SITE, link_type, id)
                })
                .to_string();
        }

        result
    }

    /// Strip WoW color coding from message.
    pub fn strip_color_coding(&self, message: &str) -> String {
        // First pass: |cFFFFFFFF...|r -> ...
        let pass1 = self.color_pattern.replace_all(message, "$1").to_string();

        // Second pass: remove any remaining |cFFFFFFFF
        self.color_end_pattern.replace_all(&pass1, "").to_string()
    }

    /// Strip texture coding from message.
    pub fn strip_texture_coding(&self, message: &str) -> String {
        self.texture_pattern.replace_all(message, "").to_string()
    }

    /// Resolve custom Discord emojis in message.
    ///
    /// Converts :emoji_name: to <:emoji_name:emoji_id> for custom server emojis.
    pub fn resolve_emojis(&self, cache: &Cache, message: &str) -> String {
        static EMOJI_PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = EMOJI_PATTERN.get_or_init(|| Regex::new(r":([a-zA-Z0-9_]+):").unwrap());

        // Build emoji map from cache
        let emoji_map: std::collections::HashMap<String, String> = cache
            .guilds()
            .iter()
            .filter_map(|guild_id| cache.guild(*guild_id))
            .flat_map(|guild| {
                guild
                    .emojis
                    .iter()
                    .map(|(id, emoji)| {
                        (
                            emoji.name.to_lowercase(),
                            format!("<:{}:{}>", emoji.name, id),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        pattern
            .replace_all(message, |caps: &regex::Captures| {
                let name = caps[1].to_lowercase();
                emoji_map
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| caps[0].to_string())
            })
            .to_string()
    }

    /// Escape Discord markdown special characters.
    pub fn escape_discord_markdown(&self, message: &str) -> String {
        message
            .replace('`', "\\`")
            .replace('*', "\\*")
            .replace('_', "\\_")
            .replace('~', "\\~")
    }

    /// Process a message from WoW for Discord.
    pub fn process_wow_to_discord(&self, cache: &Cache, message: &str) -> String {
        let step1 = self.resolve_links(message);
        let step2 = self.strip_texture_coding(&step1);
        let step3 = self.strip_color_coding(&step2);
        let step4 = self.resolve_emojis(cache, &step3);
        step4
    }

    /// Convert Discord @mentions to plain text for WoW.
    ///
    /// Converts <@123456789> to @username
    pub fn resolve_mentions_to_text(&self, message: &str, cache: &Cache) -> String {
        static MENTION_PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = MENTION_PATTERN.get_or_init(|| Regex::new(r"<@!?(\d+)>").unwrap());

        pattern
            .replace_all(message, |caps: &regex::Captures| {
                if let Ok(user_id) = caps[1].parse::<u64>() {
                    if let Some(user) = cache.user(serenity::model::id::UserId::new(user_id)) {
                        return format!("@{}", user.name);
                    }
                }
                caps[0].to_string()
            })
            .to_string()
    }

    /// Convert Discord channel mentions to plain text.
    pub fn resolve_channel_mentions(&self, message: &str, cache: &Cache) -> String {
        static CHANNEL_PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = CHANNEL_PATTERN.get_or_init(|| Regex::new(r"<#(\d+)>").unwrap());

        pattern
            .replace_all(message, |caps: &regex::Captures| {
                if let Ok(channel_id) = caps[1].parse::<u64>() {
                    let channel_id = serenity::model::id::ChannelId::new(channel_id);
                    // Search through guilds to find the channel
                    for guild_id in cache.guilds() {
                        if let Some(guild) = cache.guild(guild_id) {
                            if let Some(channel) = guild.channels.get(&channel_id) {
                                return format!("#{}", channel.name);
                            }
                        }
                    }
                }
                caps[0].to_string()
            })
            .to_string()
    }

    /// Convert Discord role mentions to plain text.
    pub fn resolve_role_mentions(&self, message: &str, cache: &Cache) -> String {
        static ROLE_PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern = ROLE_PATTERN.get_or_init(|| Regex::new(r"<@&(\d+)>").unwrap());

        pattern
            .replace_all(message, |caps: &regex::Captures| {
                if let Ok(role_id) = caps[1].parse::<u64>() {
                    // Find the role in any guild
                    for guild_id in cache.guilds() {
                        if let Some(guild) = cache.guild(guild_id) {
                            if let Some(role) =
                                guild.roles.get(&serenity::model::id::RoleId::new(role_id))
                            {
                                return format!("@{}", role.name);
                            }
                        }
                    }
                }
                caps[0].to_string()
            })
            .to_string()
    }

    /// Convert Discord custom emojis to text representation.
    pub fn resolve_custom_emojis_to_text(&self, message: &str) -> String {
        static EMOJI_PATTERN: OnceLock<Regex> = OnceLock::new();
        let pattern =
            EMOJI_PATTERN.get_or_init(|| Regex::new(r"<a?:([a-zA-Z0-9_]+):\d+>").unwrap());

        pattern.replace_all(message, ":$1:").to_string()
    }

    /// Process a message from Discord for WoW.
    pub fn process_discord_to_wow(&self, message: &str, cache: &Cache) -> String {
        let step1 = self.resolve_mentions_to_text(message, cache);
        let step2 = self.resolve_channel_mentions(&step1, cache);
        let step3 = self.resolve_role_mentions(&step2, cache);
        let step4 = self.resolve_custom_emojis_to_text(&step3);
        step4
    }
}

/// Split a long message into chunks that fit within WoW's message limit (255 chars).
pub fn split_message(format: &str, sender: &str, message: &str, time: &str) -> Vec<String> {
    let template_len = format
        .replace("%time", time)
        .replace("%user", sender)
        .replace("%message", "")
        .len();

    let max_msg_len = 255 - template_len;
    let mut result = Vec::new();
    let mut remaining = message;

    while remaining.len() > max_msg_len {
        // Try to split at a space
        let split_at = remaining[..max_msg_len].rfind(' ').unwrap_or(max_msg_len);

        result.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    if !remaining.is_empty() {
        result.push(remaining.to_string());
    }

    // Apply format to each chunk
    result
        .into_iter()
        .map(|msg| {
            let formatted = format
                .replace("%time", time)
                .replace("%user", sender)
                .replace("%message", &msg);

            // Prevent accidental dot commands
            if formatted.starts_with('.') {
                format!(" {}", formatted)
            } else {
                formatted
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_color_coding() {
        let resolver = MessageResolver::new();

        let input = "|cff00ff00Green Text|r normal";
        let output = resolver.strip_color_coding(input);
        assert_eq!(output, "Green Text normal");
    }

    #[test]
    fn test_strip_texture_coding() {
        let resolver = MessageResolver::new();

        let input = "Hello |TInterface\\Icons\\spell.blp:0|t World";
        let output = resolver.strip_texture_coding(input);
        assert_eq!(output, "Hello  World");
    }

    #[test]
    fn test_resolve_links() {
        let resolver = MessageResolver::new();

        let input = "|cff0070dd|Hitem:12345:0:0:0:0:0:0:0|h[Cool Sword]|h|r dropped!";
        let output = resolver.resolve_links(input);
        assert!(output.contains("[Cool Sword]"));
        assert!(output.contains("db.ascension.gg"));
    }

    #[test]
    fn test_escape_markdown() {
        let resolver = MessageResolver::new();

        let input = "**bold** _italic_ `code`";
        let output = resolver.escape_discord_markdown(input);
        assert_eq!(output, "\\*\\*bold\\*\\* \\_italic\\_ \\`code\\`");
    }

    #[test]
    fn test_resolve_custom_emojis_to_text() {
        let resolver = MessageResolver::new();

        let input = "Hello <:pepega:123456789> world <a:animated:987654321>";
        let output = resolver.resolve_custom_emojis_to_text(input);
        assert_eq!(output, "Hello :pepega: world :animated:");
    }

    #[test]
    fn test_split_message() {
        let format = "[%user]: %message";
        let chunks = split_message(format, "TestUser", "a".repeat(300).as_str(), "12:00");

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 255);
        }
    }
}
