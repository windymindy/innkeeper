//! Bridge orchestrator that ties WoW and Discord together.
//!
//! Manages the bidirectional message flow, formatting, and routing.
//! Filtering is handled centrally in the Bridge orchestrator.

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info};

use crate::common::{DiscordMessage, OutgoingWowMessage};
use crate::config::types::{ChannelMapping, Config, FiltersConfig};
use crate::game::formatter::{split_message, FormatContext, MessageFormatter};
use crate::game::router::{MessageRouter, SharedRouter};

use super::filter::{FilterDirection, MessageFilter};

/// The main bridge that orchestrates message flow.
pub struct Bridge {
    /// Message router.
    router: SharedRouter,
    /// Whether dot commands are enabled.
    enable_dot_commands: bool,
    /// Global message filter (applied to all messages).
    global_filter: MessageFilter,
    /// Per-channel filters keyed by Discord channel name.
    per_channel_filters: HashMap<String, MessageFilter>,
}

impl Bridge {
    /// Create a new bridge from configuration.
    pub fn new(config: &Config) -> Self {
        let router = if config.chat.channels.is_empty() {
            Arc::new(MessageRouter::empty())
        } else {
            Arc::new(MessageRouter::from_config(&config.chat))
        };

        let enable_dot_commands = config.discord.enable_dot_commands;

        // Build global filter from config
        let global_filter = build_global_filter(config.filters.as_ref());

        // Build per-channel filters from channel mappings
        let per_channel_filters = build_per_channel_filters(&config.chat.channels);

        Self {
            router,
            enable_dot_commands,
            global_filter,
            per_channel_filters,
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

    /// Process a message from Discord and prepare for WoW.
    ///
    /// Returns the messages to send to WoW, already formatted and split if needed.
    /// Messages that fail filtering are excluded from results.
    pub fn handle_discord_to_wow(&self, msg: &DiscordMessage) -> Vec<OutgoingWowMessage> {
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
            // Check for dot commands: messages starting with "." that should be sent directly
            let is_dot_command = self.enable_dot_commands && msg.content.starts_with('.');

            if is_dot_command {
                // Send the content directly without formatting
                results.push(OutgoingWowMessage {
                    chat_type: route.chat_type.to_id(),
                    channel_name: route.wow_channel_name.clone(),
                    sender: msg.sender.clone(),
                    content: msg.content.clone(),
                });
                continue;
            }

            // Get format and create formatter
            let format = route
                .discord_to_wow_format
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "%user: %message".to_string());

            let formatter = MessageFormatter::new(&format);

            // Calculate max message length and split if needed
            let max_len = formatter.max_message_length(&msg.sender, 255);
            let chunks = split_message(&msg.content, max_len);

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
                        channel_name = ?route.wow_channel_name,
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
                            channel_name = ?route.wow_channel_name,
                            "FILTERED Discord -> WoW (channel): {}",
                            formatted
                        );
                        continue;
                    }
                }

                info!(
                    chat_type = ?route.chat_type,
                    channel_name = ?route.wow_channel_name,
                    "Discord -> WoW: {}",
                    formatted
                );

                results.push(OutgoingWowMessage {
                    chat_type: route.chat_type.to_id(),
                    channel_name: route.wow_channel_name.clone(),
                    sender: msg.sender.clone(),
                    content: formatted,
                });
            }
        }

        results
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
    ) -> Vec<(String, String)> {
        let routes = self.router.get_discord_targets(chat_type, channel_name);

        if routes.is_empty() {
            debug!(chat_type, channel_name, "No Discord route for WoW message");
            return Vec::new();
        }

        let mut results = Vec::new();

        for route in routes {
            // Get format and create formatter
            let format = route
                .wow_to_discord_format
                .as_ref()
                .cloned()
                .unwrap_or_else(|| "[%user]: %message".to_string());

            let formatter = MessageFormatter::new(&format);
            let ctx = FormatContext::new(sender.unwrap_or(""), content)
                .with_target(channel_name.unwrap_or(""));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{
        ChannelMapping, ChatConfig, DiscordChannelConfig, DiscordConfig, GuildDashboardConfig,
        GuildEventsConfig, QuirksConfig, WowChannelConfig, WowConfig,
    };

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
    fn test_bridge_creation() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        assert!(bridge.enable_dot_commands);
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

        let results = bridge.handle_discord_to_wow(&msg);
        assert_eq!(results.len(), 1);
        // Dot commands are sent directly without formatting
        assert_eq!(results[0].content, ".help");
    }
}
