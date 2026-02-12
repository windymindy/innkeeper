//! Emoji, mention, and link resolution.
//!
//! Handles translation between WoW item links and Discord-friendly formats,
//! emoji resolution, and @mention handling.

use emojis;
use fancy_regex::Regex;
use serenity::cache::Cache;
use serenity::model::id::ChannelId;

use crate::common::resources::{get_achievement_name, LINK_SITE};

/// Result of resolving tags in a message.
#[derive(Debug, Clone)]
pub struct TagResolutionResult {
    /// The message with resolved tags.
    pub message: String,
    /// Any errors that occurred during tag resolution.
    pub errors: Vec<String>,
}

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
    /// Pattern for Discord user mentions (<@123> or <@!123>).
    mention_pattern: Regex,
    /// Pattern for Discord channel mentions (<#123>).
    channel_pattern: Regex,
    /// Pattern for Discord role mentions (<@&123>).
    role_pattern: Regex,
    /// Pattern for Discord custom emojis (<:name:id> or <a:name:id>).
    emoji_pattern: Regex,
    /// Patterns for @tag resolution (quoted and simple).
    tag_patterns: Vec<Regex>,
    /// Pattern for preserving Discord mentions during markdown escape.
    mention_preserve_pattern: Regex,
    /// Whether to enable markdown (disable escaping).
    enable_markdown: bool,
}

impl Default for MessageResolver {
    fn default() -> Self {
        Self::new(false)
    }
}

impl MessageResolver {
    /// Create a new message resolver with WotLK/Ascension patterns.
    pub fn new(enable_markdown: bool) -> Self {
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
            mention_pattern: Regex::new(r"<@!?(\d+)>").unwrap(),
            channel_pattern: Regex::new(r"<#(\d+)>").unwrap(),
            role_pattern: Regex::new(r"<@&(\d+)>").unwrap(),
            emoji_pattern: Regex::new(r"<a?:([a-zA-Z0-9_]+):\d+>").unwrap(),
            tag_patterns: vec![
                // Quoted tag: "@name with spaces"
                Regex::new(r#""@(.+?)""#).unwrap(),
                // Simple tag: @name
                Regex::new(r"@([\w]+)").unwrap(),
            ],
            mention_preserve_pattern: Regex::new(r"<@[&!]?\d+>").unwrap(),
            enable_markdown,
        }
    }

    /// Resolve WoW item/spell/quest links to Discord-friendly markdown.
    pub fn resolve_links(&self, message: &str) -> String {
        let mut result = message.to_string();

        for (link_type, pattern) in &self.link_patterns {
            result = pattern
                .replace_all(&result, |caps: &fancy_regex::Captures| -> String {
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
    /// Also resolves standard emojis like :smile: to 😀 if possible (optional but good for consistency).
    pub fn resolve_emojis(&self, cache: &Cache, message: &str) -> String {
        // Build emoji map from cache
        let custom_emoji_map: std::collections::HashMap<String, String> = cache
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

        let mut result = String::with_capacity(message.len() * 2);
        let mut chars = message.char_indices().peekable();

        while let Some((_idx, ch)) = chars.next() {
            if ch == ':' {
                // Potential start of emoji
                let mut end_idx = None;
                let mut shortcode = String::new();

                // Scan ahead
                while let Some(&(next_idx, next_ch)) = chars.peek() {
                    if next_ch == ':' {
                        end_idx = Some(next_idx);
                        chars.next(); // Consume closing colon
                        break;
                    } else if next_ch.is_alphanumeric()
                        || next_ch == '_'
                        || next_ch == '-'
                        || next_ch == '+'
                    {
                        shortcode.push(next_ch);
                        chars.next(); // Consume char
                    } else {
                        // Invalid char for shortcode, abort this emoji
                        break;
                    }
                }

                if let Some(_end) = end_idx {
                    if !shortcode.is_empty() {
                        let lower_shortcode = shortcode.to_lowercase();

                        // Check custom emojis first
                        if let Some(replacement) = custom_emoji_map.get(&lower_shortcode) {
                            result.push_str(replacement);
                        }
                        // Then check standard emojis
                        else if let Some(emoji) = emojis::get_by_shortcode(&lower_shortcode) {
                            result.push_str(emoji.as_str());
                        } else {
                            // Not found, keep original
                            result.push(':');
                            result.push_str(&shortcode);
                            result.push(':');
                        }
                    } else {
                        // Empty ::
                        result.push_str("::");
                    }
                } else {
                    // Incomplete or invalid, just push what we scanned
                    result.push(':');
                    result.push_str(&shortcode);
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// Escape Discord markdown special characters.
    pub fn escape_discord_markdown(&self, message: &str) -> String {
        if self.enable_markdown {
            return message.to_string();
        }
        message
            .replace('`', "\\`")
            .replace('*', "\\*")
            .replace('_', "\\_")
            .replace('~', "\\~")
    }

    /// Convert Discord @mentions to plain text for WoW.
    ///
    /// Converts <@123456789> to @username
    pub fn resolve_mentions_to_text(&self, message: &str, cache: &Cache) -> String {
        self.mention_pattern
            .replace_all(message, |caps: &fancy_regex::Captures| -> String {
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
        self.channel_pattern
            .replace_all(message, |caps: &fancy_regex::Captures| -> String {
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
        self.role_pattern
            .replace_all(message, |caps: &fancy_regex::Captures| -> String {
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
        self.emoji_pattern.replace_all(message, ":$1:").to_string()
    }

    /// Convert Unicode emojis to text aliases (e.g., 😀 -> :grinning:).
    ///
    /// Uses shortcode if available (like :joy:, :thumbsup:), otherwise falls back to name.
    pub fn resolve_unicode_emojis_to_text(&self, message: &str) -> String {
        let mut result = String::with_capacity(message.len() * 2);
        let mut chars = message.chars().peekable();

        while let Some(ch) = chars.next() {
            // Try single character emoji first
            if let Some(emoji) = emojis::get(ch.to_string().as_str()) {
                let alias = emoji.shortcode().unwrap_or_else(|| emoji.name());
                result.push(':');
                result.push_str(alias);
                result.push(':');
            } else if ch.is_ascii() {
                result.push(ch);
            } else {
                // Try with next character for multi-byte emojis
                let mut grapheme = ch.to_string();
                if let Some(&next) = chars.peek() {
                    if !next.is_ascii() {
                        grapheme.push(chars.next().unwrap_or_default());
                    }
                }

                if let Some(emoji) = emojis::get(grapheme.as_str()) {
                    let alias = emoji.shortcode().unwrap_or_else(|| emoji.name());
                    result.push(':');
                    result.push_str(alias);
                    result.push(':');
                } else {
                    result.push_str(&grapheme);
                }
            }
        }

        result
    }

    /// Process a message from Discord for WoW.
    pub fn process_discord_to_wow(&self, message: &str, cache: &Cache) -> String {
        let step1 = self.resolve_unicode_emojis_to_text(message);
        let step2 = self.resolve_mentions_to_text(&step1, cache);
        let step3 = self.resolve_channel_mentions(&step2, cache);
        let step4 = self.resolve_role_mentions(&step3, cache);
        let step5 = self.resolve_custom_emojis_to_text(&step4);
        step5
    }

    /// Pre-process WoW message content before Bridge formatting.
    ///
    /// This resolves WoW-specific formatting (links, colors, textures) that
    /// should be cleaned BEFORE the message is formatted with templates.
    /// After Bridge formatting, use `process_post_bridge()` for Discord-specific
    /// processing (emojis, tags, markdown escaping).
    pub fn process_pre_bridge(&self, message: &str) -> String {
        let step1 = self.resolve_links(message);
        let step2 = self.strip_texture_coding(&step1);
        self.strip_color_coding(&step2)
    }

    /// Post-process a formatted message for Discord.
    ///
    /// This applies Discord-specific processing after Bridge formatting:
    /// - Emoji resolution (requires cache)
    /// - Tag resolution (@mentions)
    /// - Markdown escaping (preserving mentions)
    ///
    /// Returns the processed message and any tag resolution errors.
    pub fn process_post_bridge(
        &self,
        cache: &Cache,
        channel_id: ChannelId,
        message: &str,
        self_user_id: u64,
    ) -> TagResolutionResult {
        // Resolve emojis
        let step1 = self.resolve_emojis(cache, message);

        // Resolve tags
        let tag_result = self.resolve_tags(cache, channel_id, &step1, self_user_id);

        // Escape markdown (preserving mentions)
        let final_message = self.escape_discord_markdown_preserve_mentions(&tag_result.message);

        TagResolutionResult {
            message: final_message,
            errors: tag_result.errors,
        }
    }

    /// Resolve achievement ID to a formatted link with the achievement name.
    ///
    /// Looks up the achievement name from the achievements database and formats
    /// a clickable link for Discord.
    pub fn format_achievement_link(achievement_id: u32) -> String {
        let name = get_achievement_name(achievement_id).unwrap_or("Unknown Achievement");
        format!(
            "[{}] (<{}?achievement={}>)",
            name, LINK_SITE, achievement_id
        )
    }

    /// Resolve @tags in a message to Discord mentions.
    ///
    /// Converts @tag or "@tag with spaces" in WoW messages to proper Discord
    /// `<@user_id>` or `<@&role_id>` mentions.
    ///
    /// This matches the Scala `resolveTags` behavior:
    /// - Supports `@username` and `"@username with spaces"` patterns
    /// - Matches against effective names (nicknames), usernames, and role names
    /// - Uses fuzzy matching with priority for exact matches
    /// - Reports errors when multiple matches are found
    ///
    /// Returns the message with resolved tags and any error messages.
    pub fn resolve_tags(
        &self,
        cache: &Cache,
        channel_id: ChannelId,
        message: &str,
        self_user_id: u64,
    ) -> TagResolutionResult {
        let mut errors = Vec::new();

        // Collect channel members and roles for matching
        // We need to find the guild that owns this channel
        let mut effective_names: Vec<(String, String)> = Vec::new();
        let mut user_names: Vec<(String, String)> = Vec::new();
        let mut role_names: Vec<(String, String)> = Vec::new();

        for guild_id in cache.guilds() {
            if let Some(guild) = cache.guild(guild_id) {
                // Check if this guild contains our channel
                if guild.channels.contains_key(&channel_id) {
                    // Collect members
                    for (user_id, member) in &guild.members {
                        // Skip self
                        if user_id.get() == self_user_id {
                            continue;
                        }

                        // Effective name (nickname or username)
                        let effective_name =
                            member.nick.as_ref().unwrap_or(&member.user.name).clone();
                        effective_names.push((effective_name, user_id.get().to_string()));

                        // Full username with discriminator (legacy format)
                        let full_name = format!(
                            "{}#{}",
                            member.user.name,
                            member.user.discriminator.map(|d| d.get()).unwrap_or(0)
                        );
                        user_names.push((full_name, user_id.get().to_string()));
                    }

                    // Collect roles
                    for (role_id, role) in &guild.roles {
                        if role.name != "@everyone" {
                            // Prefix with & for role mentions
                            role_names.push((role.name.clone(), format!("&{}", role_id.get())));
                        }
                    }
                    break;
                }
            }
        }

        // Process each pattern
        let mut result = message.to_string();

        for pattern in &self.tag_patterns {
            result = pattern
                .replace_all(&result, |caps: &fancy_regex::Captures| -> String {
                    let tag = &caps[1];

                    // Try to resolve the tag
                    let matches = self.resolve_tag_matcher(&effective_names, tag, false);
                    let matches = if matches.len() == 1 {
                        matches
                    } else {
                        let user_matches = self.resolve_tag_matcher(&user_names, tag, false);
                        if !matches.is_empty() && !user_matches.is_empty() {
                            if matches.len() == 1 {
                                matches
                            } else {
                                // Combine matches
                                let mut combined = matches;
                                combined.extend(user_matches);
                                combined
                            }
                        } else if matches.is_empty() {
                            user_matches
                        } else {
                            matches
                        }
                    };
                    let matches = if matches.len() == 1 {
                        matches
                    } else {
                        let role_matches = self.resolve_tag_matcher(&role_names, tag, true);
                        if matches.is_empty() {
                            role_matches
                        } else if role_matches.is_empty() || matches.len() == 1 {
                            matches
                        } else {
                            // Combine matches
                            let mut combined = matches;
                            combined.extend(role_matches);
                            combined
                        }
                    };

                    if matches.len() == 1 {
                        format!("<@{}>", matches[0].1)
                    } else if matches.len() > 1 && matches.len() < 5 {
                        let names: Vec<&str> = matches.iter().map(|(name, _)| name.as_str()).collect();
                        errors.push(format!(
                            "Your tag @{} matches multiple channel members: {}. Be more specific in your tag!",
                            tag,
                            names.join(", ")
                        ));
                        caps[0].to_string()
                    } else if matches.len() >= 5 {
                        errors.push(format!(
                            "Your tag @{} matches too many channel members. Be more specific in your tag!",
                            tag
                        ));
                        caps[0].to_string()
                    } else {
                        // No matches, leave original
                        caps[0].to_string()
                    }
                })
                .to_string();
        }

        TagResolutionResult {
            message: result,
            errors,
        }
    }

    /// Helper to match a tag against a list of (name, id) pairs.
    ///
    /// Returns matching pairs with preference for:
    /// 1. Exact matches
    /// 2. Whole-word matches
    /// 3. Substring matches
    fn resolve_tag_matcher(
        &self,
        names: &[(String, String)],
        tag: &str,
        is_role: bool,
    ) -> Vec<(String, String)> {
        let lower_tag = tag.to_lowercase();

        // Skip @here as it's a Discord keyword
        if lower_tag == "here" {
            return Vec::new();
        }

        // Find all names containing the tag as a substring
        let initial_matches: Vec<&(String, String)> = names
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&lower_tag))
            .collect();

        if initial_matches.is_empty() {
            return Vec::new();
        }

        // If we have multiple matches and the tag doesn't contain spaces, try to narrow down
        if initial_matches.len() > 1 && !lower_tag.contains(' ') {
            // First, try to find an exact match
            if let Some(exact) = initial_matches
                .iter()
                .find(|(name, _)| name.to_lowercase() == lower_tag)
            {
                return vec![(exact.0.clone(), exact.1.clone())];
            }

            // Try to find matches where the tag is a whole word in the name
            let word_matches: Vec<&(String, String)> = initial_matches
                .iter()
                .filter(|(name, _)| {
                    name.to_lowercase()
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|word| word == lower_tag)
                })
                .copied()
                .collect();

            if !word_matches.is_empty() {
                return word_matches
                    .into_iter()
                    .map(|(name, id)| {
                        let formatted_id = if is_role && !id.starts_with('&') {
                            format!("&{}", id)
                        } else {
                            id.clone()
                        };
                        (name.clone(), formatted_id)
                    })
                    .collect();
            }
        }

        // Return all initial matches
        initial_matches
            .into_iter()
            .map(|(name, id)| {
                let formatted_id = if is_role && !id.starts_with('&') {
                    format!("&{}", id)
                } else {
                    id.clone()
                };
                (name.clone(), formatted_id)
            })
            .collect()
    }

    /// Process a message from WoW for Discord.
    ///
    /// This is the full processing pipeline that includes:
    /// - Link resolution (items, spells, quests, achievements)
    /// - Texture/color stripping
    /// - Emoji resolution
    /// - Tag resolution (@mentions)
    /// - Markdown escaping
    ///
    /// Returns the processed message and any tag resolution errors.
    pub fn process_wow_to_discord(
        &self,
        cache: &Cache,
        channel_id: ChannelId,
        message: &str,
        self_user_id: u64,
    ) -> TagResolutionResult {
        // First, do the basic processing
        let step1 = self.resolve_links(message);
        let step2 = self.strip_texture_coding(&step1);
        let step3 = self.strip_color_coding(&step2);
        let step4 = self.resolve_emojis(cache, &step3);

        // Then resolve tags
        let tag_result = self.resolve_tags(cache, channel_id, &step4, self_user_id);

        // Finally escape markdown (but preserve Discord mentions)
        let final_message = self.escape_discord_markdown_preserve_mentions(&tag_result.message);

        TagResolutionResult {
            message: final_message,
            errors: tag_result.errors,
        }
    }

    /// Escape Discord markdown but preserve Discord mention syntax.
    fn escape_discord_markdown_preserve_mentions(&self, message: &str) -> String {
        if self.enable_markdown {
            return message.to_string();
        }
        // We need to escape markdown but NOT the <@id> or <@&id> mentions

        // Find all mentions and their positions
        let mut mentions: Vec<(usize, usize, String)> = Vec::new();
        let message_clone = message.to_string();

        for caps in self
            .mention_preserve_pattern
            .captures_iter(&message_clone)
            .flatten()
        {
            if let Some(m) = caps.get(0) {
                mentions.push((m.start(), m.end(), m.as_str().to_string()));
            }
        }

        if mentions.is_empty() {
            return self.escape_discord_markdown(message);
        }

        // Build result by escaping non-mention parts
        let mut result = String::new();
        let mut last_end = 0;

        for (start, end, mention) in mentions {
            // Escape the part before this mention
            if start > last_end {
                result.push_str(&self.escape_discord_markdown(&message[last_end..start]));
            }
            // Add the mention as-is
            result.push_str(&mention);
            last_end = end;
        }

        // Escape any remaining text after the last mention
        if last_end < message.len() {
            result.push_str(&self.escape_discord_markdown(&message[last_end..]));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_color_coding() {
        let resolver = MessageResolver::new(false);

        let input = "|cff00ff00Green Text|r normal";
        let output = resolver.strip_color_coding(input);
        assert_eq!(output, "Green Text normal");
    }

    #[test]
    fn test_strip_texture_coding() {
        let resolver = MessageResolver::new(false);

        let input = "Hello |TInterface\\Icons\\spell.blp:0|t World";
        let output = resolver.strip_texture_coding(input);
        assert_eq!(output, "Hello  World");
    }

    #[test]
    fn test_resolve_links() {
        let resolver = MessageResolver::new(false);

        let input = "|cff0070dd|Hitem:12345:0:0:0:0:0:0:0|h[Cool Sword]|h|r dropped!";
        let output = resolver.resolve_links(input);
        assert!(output.contains("[Cool Sword]"));
        assert!(output.contains("db.ascension.gg"));
    }

    #[test]
    fn test_escape_markdown() {
        let resolver = MessageResolver::new(false);

        let input = "**bold** _italic_ `code`";
        let output = resolver.escape_discord_markdown(input);
        assert_eq!(output, "\\*\\*bold\\*\\* \\_italic\\_ \\`code\\`");
    }

    #[test]
    fn test_resolve_custom_emojis_to_text() {
        let resolver = MessageResolver::new(false);

        let input = "Hello <:pepega:123456789> world <a:animated:987654321>";
        let output = resolver.resolve_custom_emojis_to_text(input);
        assert_eq!(output, "Hello :pepega: world :animated:");
    }

    #[test]
    fn test_resolve_unicode_emojis() {
        let resolver = MessageResolver::new(false);

        // Basic emoji conversion (😀 = grinning face)
        let input = "Hello 😀 world";
        let output = resolver.resolve_unicode_emojis_to_text(input);
        assert!(
            output.contains("grinning"),
            "Expected emoji name with 'grinning' in output, got: {}",
            output
        );

        // Multiple emojis
        let input2 = "😀😂👍";
        let output2 = resolver.resolve_unicode_emojis_to_text(input2);
        assert!(
            output2.contains("grinning"),
            "Expected :grinning, got: {}",
            output2
        );
        assert!(
            output2.contains("joy"),
            "Expected :joy emoji, got: {}",
            output2
        );
        assert!(
            output2.contains("+1"),
            "Expected :+1: (thumbs up), got: {}",
            output2
        );

        // Mixed text with emojis
        let input3 = "Hey there 🎉 party 🎊 time!";
        let output3 = resolver.resolve_unicode_emojis_to_text(input3);
        assert!(
            output3.contains("tada"),
            "Expected :tada: (party popper), got: {}",
            output3
        );
        assert!(
            output3.contains("Hey there"),
            "Original text should be preserved, got: {}",
            output3
        );
    }

    #[test]
    fn test_format_achievement_link() {
        // Test known achievement (Level 10 = ID 6)
        let output = MessageResolver::format_achievement_link(6);
        assert!(output.contains("Level 10"));
        assert!(output.contains("db.ascension.gg"));
        assert!(output.contains("achievement=6"));

        // Test unknown achievement
        let output = MessageResolver::format_achievement_link(999999999);
        assert!(output.contains("Unknown Achievement"));
    }

    #[test]
    fn test_tag_matcher_exact_match() {
        let resolver = MessageResolver::new(false);

        let names = vec![
            ("John".to_string(), "123".to_string()),
            ("Johnny".to_string(), "456".to_string()),
            ("JohnDoe".to_string(), "789".to_string()),
        ];

        // Exact match should return single result
        let matches = resolver.resolve_tag_matcher(&names, "john", false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "John");
        assert_eq!(matches[0].1, "123");
    }

    #[test]
    fn test_tag_matcher_partial_match() {
        let resolver = MessageResolver::new(false);

        let names = vec![
            ("Alice Smith".to_string(), "123".to_string()),
            ("Bob".to_string(), "456".to_string()),
        ];

        // Partial match
        let matches = resolver.resolve_tag_matcher(&names, "alice", false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "Alice Smith");
    }

    #[test]
    fn test_tag_matcher_role_prefix() {
        let resolver = MessageResolver::new(false);

        let names = vec![("Moderator".to_string(), "123".to_string())];

        // Role should get & prefix
        let matches = resolver.resolve_tag_matcher(&names, "moderator", true);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].1, "&123");
    }

    #[test]
    fn test_tag_matcher_skip_here() {
        let resolver = MessageResolver::new(false);

        let names = vec![("here".to_string(), "123".to_string())];

        // @here should be skipped
        let matches = resolver.resolve_tag_matcher(&names, "here", false);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_escape_markdown_preserve_mentions() {
        let resolver = MessageResolver::new(false);

        let input = "**bold** <@123456> text _italic_ <@&789012> more";
        let output = resolver.escape_discord_markdown_preserve_mentions(input);

        // Markdown should be escaped
        assert!(output.contains("\\*\\*bold\\*\\*"));
        assert!(output.contains("\\_italic\\_"));

        // Mentions should be preserved
        assert!(output.contains("<@123456>"));
        assert!(output.contains("<@&789012>"));
    }

    #[test]
    fn test_resolve_emojis_standard_direct() {
        // Verify direct shortcode lookup works
        assert_eq!(emojis::get_by_shortcode("smile").unwrap().as_str(), "😄");
        assert_eq!(emojis::get_by_shortcode("grinning").unwrap().as_str(), "😀");
        assert_eq!(emojis::get_by_shortcode("+1").unwrap().as_str(), "👍");
    }

    #[test]
    fn test_escape_markdown_enabled() {
        let resolver = MessageResolver::new(true);

        let input = "**bold** _italic_ `code`";
        let output = resolver.escape_discord_markdown(input);
        assert_eq!(output, input); // Should not change
    }
}
