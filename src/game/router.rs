//! Message routing between WoW and Discord.
//!
//! Handles channel mapping and bidirectional message routing based on configuration.

use std::collections::HashMap;
use std::sync::Arc;

use crate::common::types::ChatType;
use crate::config::types::{ChatConfig, WowChannelConfig};

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
        match s.to_lowercase().as_str() {
            "wow_to_discord" | "wow-to-discord" | "w2d" => Direction::WowToDiscord,
            "discord_to_wow" | "discord-to-wow" | "d2w" => Direction::DiscordToWow,
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
                wow_to_discord_format: mapping.wow.format.clone(),
                discord_to_wow_format: mapping.discord.format.clone(),
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
    use crate::config::types::{ChannelMapping, DiscordChannelConfig, WowChannelConfig};
    use crate::protocol::game::chat::chat_events;

    fn make_config(channels: Vec<ChannelMapping>) -> ChatConfig {
        ChatConfig { channels }
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
}
