//! Bridge orchestrator that ties WoW and Discord together.
//!
//! Manages the bidirectional message flow, formatting, and routing.
//! Filtering is handled centrally in the Bridge orchestrator.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info};

use crate::common::types::ChatType;
use crate::common::{BridgeMessage, DiscordMessage};
use crate::config::types::{ChannelMapping, ChatConfig, Config, FiltersConfig, WowChannelConfig};
use crate::game::formatter::{split_message, FormatContext, MessageFormatter};

use super::filter::{FilterDirection, MessageFilter};

/// The main bridge that orchestrates message flow.
pub struct Bridge {
    /// Message router.
    router: SharedRouter,
    /// Global message filter (applied to all messages).
    global_filter: MessageFilter,
    /// Per-channel filters keyed by Discord channel name.
    per_channel_filters: HashMap<String, MessageFilter>,
    /// Configuration (accessed for feature flags like enable_dot_commands and guild events).
    config: Config,
}

impl Bridge {
    /// Create a new bridge from configuration.
    pub fn new(config: &Config) -> Self {
        let router = if config.chat.channels.is_empty() {
            Arc::new(MessageRouter::empty())
        } else {
            Arc::new(MessageRouter::from_config(&config.chat))
        };

        // Build global filter from config
        let global_filter = build_global_filter(config.filters.as_ref());

        // Build per-channel filters from channel mappings
        let per_channel_filters = build_per_channel_filters(&config.chat.channels);

        Self {
            router,
            global_filter,
            per_channel_filters,
            config: config.clone(),
        }
    }

    /// Get the router for external use.
    pub fn router(&self) -> SharedRouter {
        Arc::clone(&self.router)
    }

    /// Get the list of custom channels to join in WoW.
    pub fn channels_to_join(&self) -> Vec<&str> {
        self.router.get_channels_to_join()
    }

    /// Format a command response before sending to Discord.
    ///
    /// Applies MOTD formatting if configured and the response appears to be a MOTD.
    pub fn format_command_response(&self, content: &str) -> String {
        // Check if MOTD format is configured
        if let Some(format) = self.config.get_guild_event_format("motd") {
            // Skip formatting for !who responses (they start with "**" for bold guild name)
            // or error messages (start with "No ")
            if !content.starts_with("**") && !content.starts_with("No ") {
                // This looks like a MOTD response, apply formatting
                let formatter = MessageFormatter::new(&format);
                let ctx = FormatContext::new("", content);
                return formatter.format(&ctx);
            }
        }
        content.to_string()
    }

    /// Process a dot command message from Discord and prepare for WoW.
    ///
    /// Dot commands are sent directly without formatting to the first matching route.
    /// Returns None if no route is found for the channel.
    pub fn handle_discord_to_wow_directly(&self, msg: &DiscordMessage) -> Option<BridgeMessage> {
        Some(BridgeMessage {
            chat_type: ChatType::Say.to_id(),
            channel_name: None,
            sender: None,
            content: msg.content.clone(),
            format: None,
            guild_event: None,
        })
    }

    /// Process a message from Discord and prepare for WoW.
    ///
    /// Returns the messages to send to WoW, already formatted and split if needed.
    /// Messages that fail filtering are excluded from results.
    pub fn handle_discord_to_wow(&self, msg: &DiscordMessage) -> Vec<BridgeMessage> {
        let routes = self.router.get_wow_targets(&msg.channel_name);

        if routes.is_empty() {
            debug!(
                channel_name = msg.channel_name,
                "No WoW route for Discord message"
            );
            return Vec::new();
        }

        let mut results = Vec::new();

        for route in routes {
            // Preprocess message for whisper channels ("/w <target> <message>" syntax)
            let (processed_content, whisper_target) =
                self.preprocess_whisper_message(&msg.content, route.chat_type);

            // Skip empty messages (invalid whisper format)
            if processed_content.is_empty() {
                debug!(
                    chat_type = ?route.chat_type,
                    "Skipping message - invalid whisper format or empty content"
                );
                continue;
            }

            // For whisper messages, override the channel_name with the target
            let actual_channel_name = if whisper_target.is_some() {
                whisper_target.clone()
            } else {
                route.wow_channel_name.clone()
            };

            // Get format and create formatter
            let format = route
                .discord_to_wow_format
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "%user: %message".to_string());

            let formatter = MessageFormatter::new(&format);

            // Calculate max message length and split if needed
            let max_len = formatter.max_message_length(&msg.sender, 255);
            let chunks = split_message(&processed_content, max_len);

            for chunk in chunks {
                let ctx = FormatContext::new(&msg.sender, &chunk);
                let formatted = formatter.format(&ctx);

                // Apply global filter first, then per-channel filter
                if self
                    .global_filter
                    .should_filter(FilterDirection::DiscordToWow, &formatted)
                {
                    info!(
                        chat_type = ?route.chat_type,
                        channel_name = ?actual_channel_name,
                        "FILTERED Discord -> WoW (global): {}",
                        formatted
                    );
                    continue;
                }

                // Apply per-channel filter
                if let Some(filter) = self.per_channel_filters.get(&msg.channel_name) {
                    if filter.should_filter(FilterDirection::DiscordToWow, &formatted) {
                        info!(
                            chat_type = ?route.chat_type,
                            channel_name = ?actual_channel_name,
                            "FILTERED Discord -> WoW (channel): {}",
                            formatted
                        );
                        continue;
                    }
                }

                info!(
                    chat_type = ?route.chat_type,
                    channel_name = ?actual_channel_name,
                    "Discord -> WoW: {}",
                    formatted
                );

                results.push(BridgeMessage {
                    chat_type: route.chat_type.to_id(),
                    channel_name: actual_channel_name.clone(),
                    sender: Some(msg.sender.clone()),
                    content: formatted,
                    format: None,
                    guild_event: None,
                });
            }
        }

        results
    }

    /// Preprocess a message for whisper channels.
    ///
    /// For ChatType::Whisper channels, messages must be in format `/w <target> <message>`.
    /// Returns (processed_message, whisper_target). If the message is invalid for a whisper
    /// channel, returns empty string.
    fn preprocess_whisper_message(
        &self,
        message: &str,
        chat_type: ChatType,
    ) -> (String, Option<String>) {
        // Only preprocess for Whisper type channels
        if chat_type != ChatType::Whisper {
            return (message.to_string(), None);
        }

        // Must start with "/w " (case-insensitive)
        let prefix = "/w ";
        if !message.to_lowercase().starts_with(prefix) {
            return (String::new(), None);
        }

        // Extract content after "/w "
        let after_prefix = &message[prefix.len()..];

        // Find the first space to separate target from message
        let first_space = match after_prefix.find(' ') {
            Some(pos) => pos,
            None => return (String::new(), None), // No message after target
        };

        let target = &after_prefix[..first_space];
        let actual_message = &after_prefix[first_space + 1..];

        // Validate target name: 3-12 characters, letters only
        if target.len() < 3 || target.len() > 12 {
            return (String::new(), None);
        }
        if !target.chars().all(|c| c.is_ascii_alphabetic()) {
            return (String::new(), None);
        }

        // Return the extracted message and target
        (actual_message.to_string(), Some(target.to_string()))
    }

    /// Process a message from WoW and prepare for Discord.
    ///
    /// Returns the formatted Discord messages with filtering applied.
    /// Messages that fail filtering are excluded from results.
    pub fn handle_wow_to_discord(
        &self,
        chat_type: u8,
        channel_name: Option<&str>,
        sender: Option<&str>,
        content: &str,
        format_override: Option<&str>,
        guild_event: Option<&crate::common::messages::GuildEventInfo>,
    ) -> Vec<(String, String)> {
        // Check if this is a guild event and if it's enabled
        let event_name = guild_event.as_ref().map(|e| e.event_name.as_str());
        if let Some(event_name) = event_name {
            if !self.config.is_guild_event_enabled(event_name) {
                debug!(
                    "Guild event '{}' is disabled in config, not sending to Discord",
                    event_name
                );
                return Vec::new();
            }
        }

        let routes = self.router.get_discord_targets(chat_type, channel_name);

        if routes.is_empty() {
            debug!(chat_type, channel_name, "No Discord route for WoW message");
            return Vec::new();
        }

        let mut results = Vec::new();

        for route in routes {
            // Get format (use override if provided, otherwise use config or default)
            // For guild events, look up format from guild event config
            let format = format_override
                .map(String::from)
                .or_else(|| {
                    // If this is a guild event, get format from guild event config
                    if let Some(event_name) = event_name {
                        self.config.get_guild_event_format(event_name)
                    } else {
                        None
                    }
                })
                .or_else(|| route.wow_to_discord_format.clone())
                .unwrap_or_else(|| "[%user]: %message".to_string());

            let formatter = MessageFormatter::new(&format);
            let target = guild_event
                .as_ref()
                .and_then(|e| e.target_name.as_deref())
                .unwrap_or(channel_name.unwrap_or(""));
            let rank = guild_event
                .as_ref()
                .and_then(|e| e.rank_name.as_deref())
                .unwrap_or("");
            let ctx = FormatContext::new(sender.unwrap_or(""), content)
                .with_target(target)
                .with_rank(rank);
            let formatted = formatter.format(&ctx);

            // Apply global filter first, then per-channel filter
            if self
                .global_filter
                .should_filter(FilterDirection::WowToDiscord, &formatted)
            {
                info!(
                    chat_type,
                    discord_channel = %route.discord_channel_name,
                    "FILTERED WoW -> Discord (global) [{}]: {}",
                    route.discord_channel_name,
                    formatted
                );
                continue;
            }

            // Apply per-channel filter
            if let Some(filter) = self.per_channel_filters.get(&route.discord_channel_name) {
                if filter.should_filter(FilterDirection::WowToDiscord, &formatted) {
                    info!(
                        chat_type,
                        discord_channel = %route.discord_channel_name,
                        "FILTERED WoW -> Discord (channel) [{}]: {}",
                        route.discord_channel_name,
                        formatted
                    );
                    continue;
                }
            }

            info!(
                chat_type,
                discord_channel = %route.discord_channel_name,
                "WoW -> Discord [{}]: {}",
                route.discord_channel_name,
                formatted
            );

            results.push((route.discord_channel_name.clone(), formatted));
        }

        results
    }
}

/// Build a global filter from optional config.
fn build_global_filter(filters: Option<&FiltersConfig>) -> MessageFilter {
    match filters {
        Some(f) if f.enabled => MessageFilter::new(f.patterns.clone(), f.patterns.clone()),
        _ => MessageFilter::empty(),
    }
}

/// Build per-channel filters from channel mappings.
///
/// Priority order (first non-None wins):
/// 1. Discord channel filters (highest priority - can filter both directions)
/// 2. WoW channel filters (only filter WoW -> Discord)
/// 3. Empty filter (no filtering)
fn build_per_channel_filters(channels: &[ChannelMapping]) -> HashMap<String, MessageFilter> {
    let mut filters = HashMap::new();

    for mapping in channels {
        let discord_channel = &mapping.discord.channel;

        // Discord filters take priority and apply to both directions
        if let Some(ref discord_filter) = mapping.discord.filters {
            if discord_filter.enabled {
                let filter = MessageFilter::new(
                    discord_filter.patterns.clone(),
                    discord_filter.patterns.clone(),
                );
                filters.insert(discord_channel.clone(), filter);
                continue;
            }
        }

        // WoW filters apply to WoW -> Discord only
        if let Some(ref wow_filter) = mapping.wow.filters {
            if wow_filter.enabled {
                let filter = MessageFilter::new(wow_filter.patterns.clone(), None);
                filters.insert(discord_channel.clone(), filter);
            }
        }
    }

    filters
}

/// Direction of message flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// WoW to Discord only.
    WowToDiscord,
    /// Discord to WoW only.
    DiscordToWow,
    /// Bidirectional.
    Both,
}

impl Direction {
    /// Parse direction from config string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "wow_to_discord" => Direction::WowToDiscord,
            "discord_to_wow" => Direction::DiscordToWow,
            "both" => Direction::Both,
            _ => Direction::Both,
        }
    }

    /// Check if this direction allows WoW -> Discord messages.
    pub fn allows_wow_to_discord(&self) -> bool {
        matches!(self, Direction::WowToDiscord | Direction::Both)
    }

    /// Check if this direction allows Discord -> WoW messages.
    pub fn allows_discord_to_wow(&self) -> bool {
        matches!(self, Direction::DiscordToWow | Direction::Both)
    }
}

/// Parse a ChatType from WowChannelConfig, matching Scala's parse() function.
/// Corresponds to GamePackets.ChatEvents.parse() in the Scala code.
pub fn parse_channel_config(config: &WowChannelConfig) -> (ChatType, Option<String>) {
    match config.channel_type.to_lowercase().as_str() {
        "system" => (ChatType::System, None),
        "say" => (ChatType::Say, None),
        "guild" => (ChatType::Guild, None),
        "officer" => (ChatType::Officer, None),
        "yell" => (ChatType::Yell, None),
        "emote" => (ChatType::Emote, None),
        "whisper" => (ChatType::Whisper, None),
        "whispering" => (ChatType::WhisperInform, None),
        "channel" | "custom" => (ChatType::Channel, config.channel.clone()),
        _ => {
            // For unknown types, default to custom channel with the type as name
            (ChatType::Channel, Some(config.channel_type.clone()))
        }
    }
}

/// A configured route between WoW and Discord channels.
#[derive(Debug, Clone)]
pub struct Route {
    /// WoW chat type.
    pub chat_type: ChatType,
    /// WoW channel name (for custom channels).
    pub wow_channel_name: Option<String>,
    /// Discord channel name.
    pub discord_channel_name: String,
    /// Message flow direction.
    pub direction: Direction,
    /// Format string for messages from WoW (Discord side).
    pub wow_to_discord_format: Option<String>,
    /// Format string for messages from Discord (WoW side).
    pub discord_to_wow_format: Option<String>,
}

/// Message router that handles channel mappings.
#[derive(Debug)]
pub struct MessageRouter {
    /// All configured routes.
    routes: Vec<Route>,
    /// Index: WoW channel -> Discord channels (for WoW -> Discord routing).
    wow_to_discord: HashMap<WowChannelKey, Vec<usize>>,
    /// Index: Discord channel name -> routes (for Discord -> WoW routing).
    discord_to_wow: HashMap<String, Vec<usize>>,
}

/// Key for WoW channel lookups (handles custom channel names).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WowChannelKey {
    chat_type: u8,
    channel_name: Option<String>,
}

impl WowChannelKey {
    /// Create a key from chat type and channel name.
    fn new(chat_type: ChatType, channel_name: Option<&str>) -> Self {
        WowChannelKey {
            chat_type: chat_type.to_id(),
            channel_name: channel_name.map(|s| s.to_lowercase()),
        }
    }
}

impl MessageRouter {
    /// Create a new router from configuration.
    pub fn from_config(config: &ChatConfig) -> Self {
        let mut routes = Vec::new();
        let mut wow_to_discord: HashMap<WowChannelKey, Vec<usize>> = HashMap::new();
        let mut discord_to_wow: HashMap<String, Vec<usize>> = HashMap::new();

        for mapping in &config.channels {
            let (chat_type, wow_channel_name) = parse_channel_config(&mapping.wow);

            let route = Route {
                chat_type,
                wow_channel_name: wow_channel_name.clone(),
                discord_channel_name: mapping.discord.channel.clone(),
                direction: Direction::from_str(&mapping.direction),
                // discord.format is used for messages going TO Discord (WoW → Discord)
                wow_to_discord_format: mapping.discord.format.clone(),
                // wow.format is used for messages going TO WoW (Discord → WoW)
                discord_to_wow_format: mapping.wow.format.clone(),
            };

            let idx = routes.len();
            routes.push(route.clone());

            // Build WoW -> Discord index
            if route.direction.allows_wow_to_discord() {
                let key = WowChannelKey::new(route.chat_type, route.wow_channel_name.as_deref());
                wow_to_discord.entry(key).or_default().push(idx);
            }

            // Build Discord -> WoW index
            if route.direction.allows_discord_to_wow() {
                discord_to_wow
                    .entry(route.discord_channel_name.clone())
                    .or_default()
                    .push(idx);
            }
        }

        Self {
            routes,
            wow_to_discord,
            discord_to_wow,
        }
    }

    /// Create an empty router with no routes.
    pub fn empty() -> Self {
        Self {
            routes: Vec::new(),
            wow_to_discord: HashMap::new(),
            discord_to_wow: HashMap::new(),
        }
    }

    /// Get Discord channels that should receive a message from the given WoW channel.
    pub fn get_discord_targets(&self, chat_type: u8, channel_name: Option<&str>) -> Vec<&Route> {
        let key = WowChannelKey {
            chat_type,
            channel_name: channel_name.map(|s| s.to_lowercase()),
        };

        self.wow_to_discord
            .get(&key)
            .map(|indices| indices.iter().map(|&i| &self.routes[i]).collect())
            .unwrap_or_default()
    }

    /// Get WoW channels that should receive a message from the given Discord channel.
    pub fn get_wow_targets(&self, discord_channel_name: &str) -> Vec<&Route> {
        self.discord_to_wow
            .get(discord_channel_name)
            .map(|indices| indices.iter().map(|&i| &self.routes[i]).collect())
            .unwrap_or_default()
    }

    /// Get custom channel names that need to be joined.
    pub fn get_channels_to_join(&self) -> Vec<&str> {
        self.routes
            .iter()
            .filter_map(|r| {
                if r.chat_type == ChatType::Channel {
                    r.wow_channel_name.as_deref()
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Shared router reference for use across async tasks.
pub type SharedRouter = Arc<MessageRouter>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{
        ChannelMapping, ChatConfig, DiscordChannelConfig, DiscordConfig, GuildDashboardConfig,
        GuildEventsConfig, QuirksConfig, WowChannelConfig, WowConfig,
    };
    use crate::protocol::game::chat::chat_events;

    fn make_config(channels: Vec<ChannelMapping>) -> ChatConfig {
        ChatConfig { channels }
    }

    fn make_test_config() -> Config {
        Config {
            discord: DiscordConfig {
                token: "test".to_string(),
                enable_dot_commands: true,
                dot_commands_whitelist: None,
                enable_commands_channels: None,
                enable_tag_failed_notifications: false,
            },
            wow: WowConfig {
                platform: "Mac".to_string(),
                enable_server_motd: false,
                version: "3.3.5".to_string(),
                realm_build: None,
                game_build: None,
                realmlist: "localhost:3724".to_string(),
                realm: "Test".to_string(),
                account: "test".to_string(),
                password: "test".to_string(),
                character: "TestChar".to_string(),
            },
            guild: GuildEventsConfig::default(),
            chat: ChatConfig {
                channels: vec![ChannelMapping {
                    direction: "both".to_string(),
                    wow: WowChannelConfig {
                        channel_type: "Guild".to_string(),
                        channel: None,
                        format: Some("[%user]: %message".to_string()),
                        filters: None,
                    },
                    discord: DiscordChannelConfig {
                        channel: "guild-chat".to_string(),
                        format: Some("[%user]: %message".to_string()),
                        filters: None,
                    },
                }],
            },
            filters: None,
            guild_dashboard: GuildDashboardConfig::default(),
            quirks: QuirksConfig::default(),
        }
    }

    #[test]
    fn test_direction_parsing() {
        assert_eq!(
            Direction::from_str("wow_to_discord"),
            Direction::WowToDiscord
        );
        assert_eq!(
            Direction::from_str("discord_to_wow"),
            Direction::DiscordToWow
        );
        assert_eq!(Direction::from_str("both"), Direction::Both);
        assert_eq!(Direction::from_str("invalid"), Direction::Both);
    }

    #[test]
    fn test_parse_channel_config() {
        let config = WowChannelConfig {
            channel_type: "guild".to_string(),
            channel: None,
            format: None,
            filters: None,
        };
        let (chat_type, channel_name) = parse_channel_config(&config);
        assert_eq!(chat_type, ChatType::Guild);
        assert_eq!(channel_name, None);

        let config = WowChannelConfig {
            channel_type: "GUILD".to_string(),
            channel: None,
            format: None,
            filters: None,
        };
        let (chat_type, _) = parse_channel_config(&config);
        assert_eq!(chat_type, ChatType::Guild);

        let config = WowChannelConfig {
            channel_type: "channel".to_string(),
            channel: Some("World".to_string()),
            format: None,
            filters: None,
        };
        let (chat_type, channel_name) = parse_channel_config(&config);
        assert_eq!(chat_type, ChatType::Channel);
        assert_eq!(channel_name, Some("World".to_string()));
    }

    #[test]
    fn test_router_wow_to_discord() {
        let config = make_config(vec![ChannelMapping {
            direction: "both".to_string(),
            wow: WowChannelConfig {
                channel_type: "Guild".to_string(),
                channel: None,
                format: None,
                filters: None,
            },
            discord: DiscordChannelConfig {
                channel: "guild-chat".to_string(),
                format: None,
                filters: None,
            },
        }]);

        let router = MessageRouter::from_config(&config);
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_GUILD, None);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].discord_channel_name, "guild-chat");
    }

    #[test]
    fn test_router_discord_to_wow() {
        let config = make_config(vec![ChannelMapping {
            direction: "both".to_string(),
            wow: WowChannelConfig {
                channel_type: "Officer".to_string(),
                channel: None,
                format: None,
                filters: None,
            },
            discord: DiscordChannelConfig {
                channel: "officer-chat".to_string(),
                format: None,
                filters: None,
            },
        }]);

        let router = MessageRouter::from_config(&config);
        let targets = router.get_wow_targets("officer-chat");

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].chat_type, ChatType::Officer);
    }

    #[test]
    fn test_router_direction_filtering() {
        let config = make_config(vec![ChannelMapping {
            direction: "wow_to_discord".to_string(),
            wow: WowChannelConfig {
                channel_type: "Guild".to_string(),
                channel: None,
                format: None,
                filters: None,
            },
            discord: DiscordChannelConfig {
                channel: "guild-chat".to_string(),
                format: None,
                filters: None,
            },
        }]);

        let router = MessageRouter::from_config(&config);

        // Should have WoW -> Discord route
        let wow_targets = router.get_discord_targets(chat_events::CHAT_MSG_GUILD, None);
        assert_eq!(wow_targets.len(), 1);

        // Should NOT have Discord -> WoW route
        let discord_targets = router.get_wow_targets("guild-chat");
        assert_eq!(discord_targets.len(), 0);
    }

    #[test]
    fn test_custom_channel_routing() {
        let config = make_config(vec![ChannelMapping {
            direction: "both".to_string(),
            wow: WowChannelConfig {
                channel_type: "Channel".to_string(),
                channel: Some("World".to_string()),
                format: None,
                filters: None,
            },
            discord: DiscordChannelConfig {
                channel: "world-chat".to_string(),
                format: None,
                filters: None,
            },
        }]);

        let router = MessageRouter::from_config(&config);

        // Should match with channel name
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_CHANNEL, Some("World"));
        assert_eq!(targets.len(), 1);

        // Case insensitive
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_CHANNEL, Some("world"));
        assert_eq!(targets.len(), 1);

        // Different channel name should not match
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_CHANNEL, Some("Trade"));
        assert_eq!(targets.len(), 0);
    }

    #[test]
    fn test_get_channels_to_join() {
        let config = make_config(vec![
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Guild".to_string(),
                    channel: None,
                    format: None,
                    filters: None,
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat".to_string(),
                    format: None,
                    filters: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Channel".to_string(),
                    channel: Some("World".to_string()),
                    format: None,
                    filters: None,
                },
                discord: DiscordChannelConfig {
                    channel: "world-chat".to_string(),
                    format: None,
                    filters: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Channel".to_string(),
                    channel: Some("Trade".to_string()),
                    format: None,
                    filters: None,
                },
                discord: DiscordChannelConfig {
                    channel: "trade-chat".to_string(),
                    format: None,
                    filters: None,
                },
            },
        ]);

        let router = MessageRouter::from_config(&config);
        let channels = router.get_channels_to_join();

        assert_eq!(channels.len(), 2);
        assert!(channels.contains(&"World"));
        assert!(channels.contains(&"Trade"));
    }

    #[test]
    fn test_multiple_discord_channels() {
        let config = make_config(vec![
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Guild".to_string(),
                    channel: None,
                    format: None,
                    filters: None,
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat-1".to_string(),
                    format: None,
                    filters: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Guild".to_string(),
                    channel: None,
                    format: None,
                    filters: None,
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat-2".to_string(),
                    format: None,
                    filters: None,
                },
            },
        ]);

        let router = MessageRouter::from_config(&config);
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_GUILD, None);

        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn test_bridge_creation() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        assert!(bridge.config.discord.enable_dot_commands);
        assert!(bridge.channels_to_join().is_empty()); // "guild" is not a custom channel
    }

    #[test]
    fn test_discord_to_wow_processing() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        let msg = DiscordMessage {
            sender: "Player".to_string(),
            content: "Hello world!".to_string(),
            channel_id: 123456789,
            channel_name: "guild-chat".to_string(),
        };

        let results = bridge.handle_discord_to_wow(&msg);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "[Player]: Hello world!");
    }

    #[test]
    fn test_dot_command_passthrough() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        let msg = DiscordMessage {
            sender: "Player".to_string(),
            content: ".help".to_string(),
            channel_id: 123456789,
            channel_name: "guild-chat".to_string(),
        };

        let result = bridge.handle_discord_to_wow_directly(&msg);
        assert!(result.is_some());
        // Dot commands are sent directly without formatting
        assert_eq!(result.unwrap().content, ".help");
    }
}
