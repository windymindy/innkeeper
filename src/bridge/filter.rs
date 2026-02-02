//! Message filtering with regex patterns.
//!
//! Filters messages based on configurable regex patterns to prevent
//! spam or unwanted messages from being relayed between Discord and WoW.

use fancy_regex::Regex;
use tracing::warn;

/// Direction of message flow for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDirection {
    /// WoW to Discord.
    WowToDiscord,
    /// Discord to WoW.
    DiscordToWow,
}

/// Message filter that checks messages against regex patterns.
#[derive(Debug, Clone)]
pub struct MessageFilter {
    /// Compiled patterns for WoW -> Discord filtering.
    wow_to_discord_patterns: Vec<CompiledPattern>,
    /// Compiled patterns for Discord -> WoW filtering.
    discord_to_wow_patterns: Vec<CompiledPattern>,
}

/// A compiled regex pattern with its original string for debugging.
#[derive(Debug, Clone)]
struct CompiledPattern {
    original: String,
    regex: Regex,
}

impl MessageFilter {
    /// Create a new message filter from pattern strings.
    ///
    /// Invalid regex patterns are logged and skipped.
    pub fn new(wow_to_discord: Option<Vec<String>>, discord_to_wow: Option<Vec<String>>) -> Self {
        Self {
            wow_to_discord_patterns: compile_patterns(wow_to_discord.unwrap_or_default()),
            discord_to_wow_patterns: compile_patterns(discord_to_wow.unwrap_or_default()),
        }
    }

    /// Create an empty filter that allows all messages.
    pub fn empty() -> Self {
        Self {
            wow_to_discord_patterns: Vec::new(),
            discord_to_wow_patterns: Vec::new(),
        }
    }

    /// Check if a message should be filtered (blocked) for the given direction.
    ///
    /// Returns `true` if the message matches any filter pattern and should be blocked.
    pub fn should_filter(&self, direction: FilterDirection, message: &str) -> bool {
        let patterns = match direction {
            FilterDirection::WowToDiscord => &self.wow_to_discord_patterns,
            FilterDirection::DiscordToWow => &self.discord_to_wow_patterns,
        };

        patterns.iter().any(|p| {
            p.regex.is_match(message).unwrap_or_else(|e| {
                warn!("Regex match error for pattern '{}': {}", p.original, e);
                false
            })
        })
    }

    /// Returns true if the filter has any patterns configured.
    pub fn has_patterns(&self) -> bool {
        !self.wow_to_discord_patterns.is_empty() || !self.discord_to_wow_patterns.is_empty()
    }
}

/// Compile a list of regex pattern strings, skipping invalid ones.
fn compile_patterns(patterns: Vec<String>) -> Vec<CompiledPattern> {
    patterns
        .into_iter()
        .filter_map(|pattern| match Regex::new(&pattern) {
            Ok(regex) => Some(CompiledPattern {
                original: pattern,
                regex,
            }),
            Err(e) => {
                warn!("Invalid filter regex pattern '{}': {}", pattern, e);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_filter_allows_all() {
        let filter = MessageFilter::empty();
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "any message"));
        assert!(!filter.should_filter(FilterDirection::DiscordToWow, "any message"));
    }

    #[test]
    fn test_exact_match_filter() {
        let filter = MessageFilter::new(Some(vec!["^spam$".to_string()]), None);
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "spam"));
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "not spam"));
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "spam message"));
    }

    #[test]
    fn test_partial_match_filter() {
        let filter = MessageFilter::new(Some(vec!["gold.*sell".to_string()]), None);
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "Buy gold selling cheap!"));
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "gold sell"));
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "I need gold"));
    }

    #[test]
    fn test_multiple_patterns() {
        let filter = MessageFilter::new(
            Some(vec![
                "spam".to_string(),
                "gold.*sell".to_string(),
                "www\\..*\\.com".to_string(),
            ]),
            None,
        );
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "this is spam"));
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "gold selling"));
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "visit www.scam.com"));
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "hello world"));
    }

    #[test]
    fn test_discord_to_wow_filter() {
        let filter = MessageFilter::new(None, Some(vec!["blocked".to_string()]));
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "blocked"));
        assert!(filter.should_filter(FilterDirection::DiscordToWow, "blocked"));
    }

    #[test]
    fn test_invalid_regex_skipped() {
        // Invalid regex should be skipped without panicking
        let filter = MessageFilter::new(
            Some(vec![
                "[invalid".to_string(), // Invalid regex
                "valid".to_string(),    // Valid regex
            ]),
            None,
        );
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "valid pattern"));
        // Filter still works with valid patterns
    }

    #[test]
    fn test_case_sensitivity() {
        let filter = MessageFilter::new(
            Some(vec!["(?i)spam".to_string()]), // Case insensitive
            None,
        );
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "SPAM"));
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "Spam"));
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "spam"));
    }

    #[test]
    fn test_negative_lookahead() {
        // Pattern that matches "wtb" followed by "dp" but NOT if "wts" appears before "dp"
        let filter = MessageFilter::new(Some(vec!["(?i).*wtb(((?!wts).)*)dp.*".to_string()]), None);
        // Should match: wtb with dp, no wts
        assert!(filter.should_filter(FilterDirection::WowToDiscord, "wtb any dp"));
        // Should NOT match: has wts
        assert!(!filter.should_filter(FilterDirection::WowToDiscord, "wtb wts dp"));
    }
}
