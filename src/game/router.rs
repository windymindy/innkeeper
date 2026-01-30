//! Message routing between WoW and Discord.
//!
//! Handles channel mapping and bidirectional message routing based on configuration.

use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::ChannelId;

use crate::config::types::{ChannelMapping, ChatConfig, WowChannelConfig};
use crate::protocol::game::chat::chat_events;

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

/// WoW channel identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WowChannel {
    /// Guild chat.
    Guild,
    /// Officer chat.
    Officer,
    /// Say (local) chat.
    Say,
    /// Yell chat.
    Yell,
    /// Emote.
    Emote,
    /// Whisper to/from a specific player.
    Whisper,
    /// Custom channel (e.g., "World", "Trade").
    Custom(String),
    /// System message.
    System,
    /// Achievement.
    Achievement,
    /// Guild achievement.
    GuildAchievement,
}

impl WowChannel {
    /// Parse a WoW channel from config string.
    pub fn from_config(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "guild" => WowChannel::Guild,
            "officer" => WowChannel::Officer,
            "say" => WowChannel::Say,
            "yell" => WowChannel::Yell,
            "emote" => WowChannel::Emote,
            "whisper" => WowChannel::Whisper,
            "system" => WowChannel::System,
            "achievement" => WowChannel::Achievement,
            "guild_achievement" => WowChannel::GuildAchievement,
            _ => WowChannel::Custom(s.to_string()),
        }
    }

    /// Parse a WoW channel from WowChannelConfig.
    pub fn from_channel_config(config: &WowChannelConfig) -> Self {
        match config.channel_type.to_lowercase().as_str() {
            "guild" => WowChannel::Guild,
            "officer" => WowChannel::Officer,
            "say" => WowChannel::Say,
            "yell" => WowChannel::Yell,
            "emote" => WowChannel::Emote,
            "whisper" => WowChannel::Whisper,
            "system" => WowChannel::System,
            "achievement" => WowChannel::Achievement,
            "guild_achievement" => WowChannel::GuildAchievement,
            "channel" => {
                // For custom channels, use the channel name if provided
                if let Some(ref name) = config.channel {
                    WowChannel::Custom(name.clone())
                } else {
                    WowChannel::Custom("Unknown".to_string())
                }
            }
            _ => {
                // For unknown types, use channel name if available
                if let Some(ref name) = config.channel {
                    WowChannel::Custom(name.clone())
                } else {
                    WowChannel::Custom(config.channel_type.clone())
                }
            }
        }
    }

    /// Get the chat message type byte for this channel.
    pub fn to_chat_type(&self) -> u8 {
        match self {
            WowChannel::Guild => chat_events::CHAT_MSG_GUILD,
            WowChannel::Officer => chat_events::CHAT_MSG_OFFICER,
            WowChannel::Say => chat_events::CHAT_MSG_SAY,
            WowChannel::Yell => chat_events::CHAT_MSG_YELL,
            WowChannel::Emote => chat_events::CHAT_MSG_EMOTE,
            WowChannel::Whisper => chat_events::CHAT_MSG_WHISPER,
            WowChannel::Custom(_) => chat_events::CHAT_MSG_CHANNEL,
            WowChannel::System => chat_events::CHAT_MSG_SYSTEM,
            WowChannel::Achievement => chat_events::CHAT_MSG_ACHIEVEMENT,
            WowChannel::GuildAchievement => chat_events::CHAT_MSG_GUILD_ACHIEVEMENT,
        }
    }

    /// Create from chat type byte and optional channel name.
    pub fn from_chat_type(chat_type: u8, channel_name: Option<&str>) -> Self {
        match chat_type {
            chat_events::CHAT_MSG_GUILD => WowChannel::Guild,
            chat_events::CHAT_MSG_OFFICER => WowChannel::Officer,
            chat_events::CHAT_MSG_SAY => WowChannel::Say,
            chat_events::CHAT_MSG_YELL => WowChannel::Yell,
            chat_events::CHAT_MSG_EMOTE | chat_events::CHAT_MSG_TEXT_EMOTE => WowChannel::Emote,
            chat_events::CHAT_MSG_WHISPER | chat_events::CHAT_MSG_WHISPER_INFORM => {
                WowChannel::Whisper
            }
            chat_events::CHAT_MSG_CHANNEL => {
                if let Some(name) = channel_name {
                    WowChannel::Custom(name.to_string())
                } else {
                    WowChannel::Custom("Unknown".to_string())
                }
            }
            chat_events::CHAT_MSG_SYSTEM => WowChannel::System,
            chat_events::CHAT_MSG_ACHIEVEMENT => WowChannel::Achievement,
            chat_events::CHAT_MSG_GUILD_ACHIEVEMENT => WowChannel::GuildAchievement,
            _ => WowChannel::System, // Default to system for unknown types
        }
    }

    /// Get the channel name if this is a custom channel.
    pub fn channel_name(&self) -> Option<&str> {
        match self {
            WowChannel::Custom(name) => Some(name),
            _ => None,
        }
    }
}

/// A configured route between WoW and Discord channels.
#[derive(Debug, Clone)]
pub struct Route {
    /// WoW channel.
    pub wow_channel: WowChannel,
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

impl From<&WowChannel> for WowChannelKey {
    fn from(channel: &WowChannel) -> Self {
        WowChannelKey {
            chat_type: channel.to_chat_type(),
            channel_name: channel.channel_name().map(|s| s.to_lowercase()),
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
            let route = Route {
                wow_channel: WowChannel::from_channel_config(&mapping.wow),
                discord_channel_name: mapping.discord.channel.clone(),
                direction: Direction::from_str(&mapping.direction),
                wow_to_discord_format: mapping.wow.format.clone(),
                discord_to_wow_format: mapping.discord.format.clone(),
            };

            let idx = routes.len();
            routes.push(route.clone());

            // Build WoW -> Discord index
            if route.direction.allows_wow_to_discord() {
                let key = WowChannelKey::from(&route.wow_channel);
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

    /// Check if a Discord channel is mapped for sending to WoW.
    pub fn has_discord_mapping(&self, discord_channel_name: &str) -> bool {
        self.discord_to_wow.contains_key(discord_channel_name)
    }

    /// Check if a WoW channel is mapped for sending to Discord.
    pub fn has_wow_mapping(&self, chat_type: u8, channel_name: Option<&str>) -> bool {
        let key = WowChannelKey {
            chat_type,
            channel_name: channel_name.map(|s| s.to_lowercase()),
        };
        self.wow_to_discord.contains_key(&key)
    }

    /// Get all routes.
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Get custom channel names that need to be joined.
    pub fn get_channels_to_join(&self) -> Vec<&str> {
        self.routes
            .iter()
            .filter_map(|r| r.wow_channel.channel_name())
            .collect()
    }
}

/// Shared router reference for use across async tasks.
pub type SharedRouter = Arc<MessageRouter>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{DiscordChannelConfig, WowChannelConfig};

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
    fn test_wow_channel_parsing() {
        assert_eq!(WowChannel::from_config("guild"), WowChannel::Guild);
        assert_eq!(WowChannel::from_config("GUILD"), WowChannel::Guild);
        assert_eq!(WowChannel::from_config("officer"), WowChannel::Officer);
        assert_eq!(
            WowChannel::from_config("World"),
            WowChannel::Custom("World".to_string())
        );
    }

    #[test]
    fn test_router_wow_to_discord() {
        let config = make_config(vec![ChannelMapping {
            direction: "both".to_string(),
            wow: WowChannelConfig {
                channel_type: "Guild".to_string(),
                channel: None,
                format: None,
            },
            discord: DiscordChannelConfig {
                channel: "guild-chat".to_string(),
                format: None,
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
            },
            discord: DiscordChannelConfig {
                channel: "officer-chat".to_string(),
                format: None,
            },
        }]);

        let router = MessageRouter::from_config(&config);
        let targets = router.get_wow_targets("officer-chat");

        assert_eq!(targets.len(), 1);
        assert!(matches!(targets[0].wow_channel, WowChannel::Officer));
    }

    #[test]
    fn test_router_direction_filtering() {
        let config = make_config(vec![ChannelMapping {
            direction: "wow_to_discord".to_string(),
            wow: WowChannelConfig {
                channel_type: "Guild".to_string(),
                channel: None,
                format: None,
            },
            discord: DiscordChannelConfig {
                channel: "guild-chat".to_string(),
                format: None,
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
            },
            discord: DiscordChannelConfig {
                channel: "world-chat".to_string(),
                format: None,
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
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat".to_string(),
                    format: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Channel".to_string(),
                    channel: Some("World".to_string()),
                    format: None,
                },
                discord: DiscordChannelConfig {
                    channel: "world-chat".to_string(),
                    format: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Channel".to_string(),
                    channel: Some("Trade".to_string()),
                    format: None,
                },
                discord: DiscordChannelConfig {
                    channel: "trade-chat".to_string(),
                    format: None,
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
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat-1".to_string(),
                    format: None,
                },
            },
            ChannelMapping {
                direction: "both".to_string(),
                wow: WowChannelConfig {
                    channel_type: "Guild".to_string(),
                    channel: None,
                    format: None,
                },
                discord: DiscordChannelConfig {
                    channel: "guild-chat-2".to_string(),
                    format: None,
                },
            },
        ]);

        let router = MessageRouter::from_config(&config);
        let targets = router.get_discord_targets(chat_events::CHAT_MSG_GUILD, None);

        assert_eq!(targets.len(), 2);
    }
}
