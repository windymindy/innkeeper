//! Bridge orchestrator that ties WoW and Discord together.
//!
//! Manages the bidirectional message flow, formatting, and routing.
//! Filtering is now handled per-route in the MessageRouter.

use std::sync::Arc;

use tracing::{debug, info};

use crate::common::{DiscordMessage, OutgoingWowMessage};
use crate::config::types::Config;
use crate::discord::resolver::MessageResolver;
use crate::game::formatter::{split_message, FormatContext, MessageFormatter};
use crate::game::router::{MessageRouter, SharedRouter};

/// The main bridge that orchestrates message flow.
pub struct Bridge {
    /// Message router.
    router: SharedRouter,
    /// Message resolver for Discord transformations.
    resolver: Arc<MessageResolver>,
    /// Whether dot commands are enabled.
    enable_dot_commands: bool,
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

        Self {
            router,
            resolver: Arc::new(MessageResolver::new()),
            enable_dot_commands,
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

                // Apply per-route filter
                if route.filter.should_filter_discord_to_wow(&formatted) {
                    info!(
                        chat_type = ?route.chat_type,
                        channel_name = ?route.wow_channel_name,
                        "FILTERED Discord->WoW: {}",
                        formatted
                    );
                    continue;
                }

                info!(
                    chat_type = ?route.chat_type,
                    channel_name = ?route.wow_channel_name,
                    "Discord->WoW: {}",
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
