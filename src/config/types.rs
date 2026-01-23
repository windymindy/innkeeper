//! Configuration type definitions.

use serde::Deserialize;

/// Root configuration structure.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub wow: WowConfig,
    pub discord: DiscordConfig,
    pub guild: Option<GuildConfig>,
    pub chat: Option<ChatConfig>,
    pub filters: Option<FiltersConfig>,
}

/// WoW server connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct WowConfig {
    pub realm: RealmConfig,
    pub account: AccountConfig,
    pub character: String,
}

/// Realm server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RealmConfig {
    pub host: String,
    pub port: u16,
    pub name: String,
}

/// Account credentials.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountConfig {
    pub username: String,
    pub password: String,
}

/// Discord bot configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscordConfig {
    pub token: String,
    pub guild_id: Option<u64>,
    pub enable_dot_commands: Option<bool>,
}

/// Guild-specific settings.
#[derive(Debug, Clone, Deserialize)]
pub struct GuildConfig {
    pub online_channel: Option<u64>,
    pub achievement_channel: Option<u64>,
}

/// Chat channel mappings.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatConfig {
    pub channels: Vec<ChannelMapping>,
}

/// Maps a WoW channel to a Discord channel.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelMapping {
    /// WoW channel type: "guild", "officer", "say", or a custom channel name
    pub wow: String,
    /// Discord channel ID
    pub discord: u64,
    /// Direction: "both", "wow_to_discord", "discord_to_wow"
    pub direction: Option<String>,
    /// Format string for messages
    pub format: Option<String>,
}

/// Message filtering configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct FiltersConfig {
    /// Patterns to filter out from WoW messages
    pub wow_to_discord: Option<Vec<String>>,
    /// Patterns to filter out from Discord messages
    pub discord_to_wow: Option<Vec<String>>,
}
