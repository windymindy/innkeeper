//! Configuration type definitions.

use serde::{Deserialize, Deserializer};

/// Deserialize a boolean from either a bool or an integer (1=true, 0=false).
fn bool_or_int<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    struct BoolOrIntVisitor;

    impl<'de> serde::de::Visitor<'de> for BoolOrIntVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a boolean (true/false) or integer (1/0)")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            match value {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(E::custom(format!("expected 0 or 1, got {}", value))),
            }
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            match value {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(E::custom(format!("expected 0 or 1, got {}", value))),
            }
        }
    }

    deserializer.deserialize_any(BoolOrIntVisitor)
}

/// Deserialize an Option<String> that handles both Some(value) and None cases.
fn option_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    struct OptionStringVisitor;

    impl<'de> serde::de::Visitor<'de> for OptionStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or null")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(value))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptionStringVisitor)
}

/// Deserialize an Option<u32> that handles both Some(value) and None cases.
fn option_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    struct OptionU32Visitor;

    impl<'de> serde::de::Visitor<'de> for OptionU32Visitor {
        type Value = Option<u32>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an unsigned integer or null")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(value as u32))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if value >= 0 {
                Ok(Some(value as u32))
            } else {
                Err(E::custom("expected non-negative integer"))
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptionU32Visitor)
}

/// Deserialize an Option<Vec<String>> that handles both Some(value) and None cases.
fn option_vec_string<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    struct OptionVecStringVisitor;

    impl<'de> serde::de::Visitor<'de> for OptionVecStringVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of strings or null")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                vec.push(value);
            }
            Ok(Some(vec))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptionVecStringVisitor)
}

/// Deserialize an Option<T> for struct types that handles both Some(value) and None cases.
fn option_struct<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    use serde::de::Error;

    struct OptionStructVisitor<T> {
        _phantom: std::marker::PhantomData<T>,
    }

    impl<'de, T> serde::de::Visitor<'de> for OptionStructVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a config object or null")
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let config = T::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
            Ok(Some(config))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(OptionStructVisitor {
        _phantom: std::marker::PhantomData,
    })
}

/// Root configuration structure.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub discord: DiscordConfig,
    pub wow: WowConfig,
    #[serde(default)]
    pub guild: GuildEventsConfig,
    #[serde(default)]
    pub chat: ChatConfig,
    #[serde(default, deserialize_with = "option_struct")]
    pub filters: Option<FiltersConfig>,
    #[serde(rename = "guild-dashboard", default)]
    pub guild_dashboard: GuildDashboardConfig,
    #[serde(default)]
    pub quirks: QuirksConfig,
}

fn default_enabled() -> bool {
    true
}

fn default_disabled() -> bool {
    false
}

fn default_empty_string() -> String {
    String::new()
}

/// Discord bot configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscordConfig {
    /// Discord bot token (or use DISCORD_TOKEN env var)
    #[serde(default)]
    pub token: String,
    /// Enable dot commands (.help, etc.)
    #[serde(default = "default_enabled", deserialize_with = "bool_or_int")]
    pub enable_dot_commands: bool,
    /// Whitelist of enabled dot commands (empty = all enabled based on enable_dot_commands)
    #[serde(default, deserialize_with = "option_vec_string")]
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// Discord channels where commands are enabled (empty = all channels)
    #[serde(default, deserialize_with = "option_vec_string")]
    pub enable_commands_channels: Option<Vec<String>>,
    /// Notify on failed tag/mention resolution
    #[serde(default = "default_enabled", deserialize_with = "bool_or_int")]
    pub enable_tag_failed_notifications: bool,
}

/// WoW server connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct WowConfig {
    /// Platform: Windows or Mac (Mac required for Warden-enabled servers)
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Treat server's MotD as SYSTEM message
    #[serde(default = "default_enabled", deserialize_with = "bool_or_int")]
    pub enable_server_motd: bool,
    /// Game version (e.g., "3.3.5", "1.12.1")
    #[serde(default = "default_version")]
    pub version: String,
    /// Realm build number (optional, for specific versions)
    #[serde(default, deserialize_with = "option_u32")]
    pub realm_build: Option<u32>,
    /// Game build number (optional, for specific versions)
    #[serde(default, deserialize_with = "option_u32")]
    pub game_build: Option<u32>,
    /// Realm list server address (realmlist)
    pub realmlist: String,
    /// Realm name to connect to
    pub realm: String,
    /// Account username (or use WOW_ACCOUNT env var)
    pub account: String,
    /// Account password (or use WOW_PASSWORD env var)
    pub password: String,
    /// Character name to login with
    pub character: String,
}

fn default_platform() -> String {
    "Mac".to_string()
}

fn default_version() -> String {
    "3.3.5".to_string()
}

/// Guild event notification configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GuildEventsConfig {
    #[serde(default, deserialize_with = "option_struct")]
    pub online: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub offline: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub promoted: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub demoted: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub joined: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub left: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub removed: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub motd: Option<GuildEventConfig>,
    #[serde(default, deserialize_with = "option_struct")]
    pub achievement: Option<GuildEventConfig>,
}

/// Individual guild event configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct GuildEventConfig {
    /// Whether this event is enabled
    #[serde(default = "default_enabled", deserialize_with = "bool_or_int")]
    pub enabled: bool,
    /// Format string for the event message
    #[serde(default, deserialize_with = "option_string")]
    pub format: Option<String>,
}

/// Chat channel mappings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChatConfig {
    #[serde(default)]
    pub channels: Vec<ChannelMapping>,
}

/// Maps a WoW channel to a Discord channel.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelMapping {
    /// Message direction: "both", "wow_to_discord", "discord_to_wow"
    #[serde(default = "default_direction")]
    pub direction: String,
    /// WoW channel configuration
    pub wow: WowChannelConfig,
    /// Discord channel configuration
    pub discord: DiscordChannelConfig,
}

fn default_direction() -> String {
    "both".to_string()
}

/// WoW channel configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct WowChannelConfig {
    /// Channel type: Guild, Officer, Say, Yell, Emote, System, Channel, Whisper
    #[serde(rename = "type")]
    pub channel_type: String,
    /// Channel name (for custom channels)
    #[serde(default, deserialize_with = "option_string")]
    pub channel: Option<String>,
    /// Format string for messages from WoW
    #[serde(default, deserialize_with = "option_string")]
    pub format: Option<String>,
    /// Per-channel filter configuration for WoW -> Discord messages
    #[serde(default, deserialize_with = "option_struct")]
    pub filters: Option<FiltersConfig>,
}

/// Discord channel configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscordChannelConfig {
    /// Discord channel name (not ID)
    pub channel: String,
    /// Format string for messages from Discord
    #[serde(default, deserialize_with = "option_string")]
    pub format: Option<String>,
    /// Per-channel filter configuration for Discord -> WoW and WoW -> Discord messages
    #[serde(default, deserialize_with = "option_struct")]
    pub filters: Option<FiltersConfig>,
}

/// Message filtering configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct FiltersConfig {
    /// Whether filtering is enabled
    #[serde(default = "default_enabled", deserialize_with = "bool_or_int")]
    pub enabled: bool,
    /// Regex patterns to filter (Java regex syntax)
    #[serde(default, deserialize_with = "option_vec_string")]
    pub patterns: Option<Vec<String>>,
}

/// Guild dashboard configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GuildDashboardConfig {
    /// Whether the guild dashboard is enabled
    #[serde(default = "default_disabled", deserialize_with = "bool_or_int")]
    pub enabled: bool,
    /// Discord channel name for online member list
    #[serde(default = "default_empty_string")]
    pub channel: String,
}

/// Server quirks configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QuirksConfig {
    #[serde(default = "default_disabled", deserialize_with = "bool_or_int")]
    pub sit: bool,
}

impl Config {
    /// Get the realmlist host and port.
    /// If realmlist contains a port, it's extracted. Otherwise defaults to 3724.
    pub fn get_realm_host_port(&self) -> (String, u16) {
        let realmlist = &self.wow.realmlist;
        if let Some(colon_pos) = realmlist.rfind(':') {
            let host = &realmlist[..colon_pos];
            if let Ok(port) = realmlist[colon_pos + 1..].parse::<u16>() {
                return (host.to_string(), port);
            }
        }
        (realmlist.clone(), 3724)
    }

    /// Check if dot commands are enabled.
    pub fn dot_commands_enabled(&self) -> bool {
        self.discord.enable_dot_commands
    }

    /// Check if tag failed notifications are enabled.
    pub fn tag_failed_notifications_enabled(&self) -> bool {
        self.discord.enable_tag_failed_notifications
    }

    /// Check if server MotD should be treated as system message.
    pub fn server_motd_enabled(&self) -> bool {
        self.wow.enable_server_motd
    }

    /// Check if guild event is enabled.
    pub fn is_guild_event_enabled(&self, event: &str) -> bool {
        let event_config = match event {
            "online" => &self.guild.online,
            "offline" => &self.guild.offline,
            "promoted" => &self.guild.promoted,
            "demoted" => &self.guild.demoted,
            "joined" => &self.guild.joined,
            "left" => &self.guild.left,
            "removed" => &self.guild.removed,
            "motd" => &self.guild.motd,
            "achievement" => &self.guild.achievement,
            _ => &None,
        };
        if let Some(ref config) = event_config {
            return config.enabled;
        }
        false
    }

    /// Get format for a guild event.
    pub fn get_guild_event_format(&self, event: &str) -> Option<String> {
        let event_config = match event {
            "online" => &self.guild.online,
            "offline" => &self.guild.offline,
            "promoted" => &self.guild.promoted,
            "demoted" => &self.guild.demoted,
            "joined" => &self.guild.joined,
            "left" => &self.guild.left,
            "removed" => &self.guild.removed,
            "motd" => &self.guild.motd,
            "achievement" => &self.guild.achievement,
            _ => &None,
        };
        if let Some(ref config) = event_config {
            return config.format.clone();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parser::load_config_str;

    #[test]
    fn test_bool_or_int_deserializer_integers() {
        let config_str = r#"
            discord {
                token="test"
                enable_dot_commands=1
                enable_tag_failed_notifications=0
            }
            wow {
                platform=Mac
                enable_server_motd=1
                version=3.3.5
                realmlist=localhost
                realm=Test
                account=testuser
                password=testpass
                character=TestChar
            }
            guild {
                online {
                    enabled=1
                }
            }
            filters {
                enabled=0
            }
        "#;

        let config = load_config_str(config_str).expect("Should parse integer booleans");
        assert!(config.discord.enable_dot_commands);
        assert!(!config.discord.enable_tag_failed_notifications);
        assert!(config.wow.enable_server_motd);
        assert!(config.is_guild_event_enabled("online"));
        assert!(!config.filters.unwrap().enabled);
    }

    #[test]
    fn test_bool_or_int_deserializer_booleans() {
        let config_str = r#"
            discord {
                token="test"
                enable_dot_commands=true
                enable_tag_failed_notifications=false
            }
            wow {
                platform=Mac
                enable_server_motd=true
                version=3.3.5
                realmlist=localhost
                realm=Test
                account=testuser
                password=testpass
                character=TestChar
            }
            guild {
                online {
                    enabled=true
                }
            }
            filters {
                enabled=false
            }
        "#;

        let config = load_config_str(config_str).expect("Should parse boolean values");
        assert!(config.discord.enable_dot_commands);
        assert!(!config.discord.enable_tag_failed_notifications);
        assert!(config.wow.enable_server_motd);
        assert!(config.is_guild_event_enabled("online"));
        assert!(!config.filters.unwrap().enabled);
    }
}
