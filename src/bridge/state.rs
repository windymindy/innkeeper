//! Bridge state management.
//!
//! Provides state types for the Discord-WoW bridge initialization lifecycle:
//! - `PendingBridgeState`: Configuration waiting for Discord channel resolution
//! - `ResolvedBridgeState`: Fully resolved state, immutable after creation
//!
//! The initialization flow is:
//! 1. Create `PendingBridgeState` with channel configs
//! 2. Wait for Discord `ready()` to get HTTP client and user ID
//! 3. Wait for `guild_create()` to resolve channel names to IDs
//! 4. Build `ResolvedBridgeState` and spawn background tasks with owned copies

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serenity::http::Http;
use serenity::model::channel::GuildChannel;
use serenity::model::id::ChannelId;
use tokio::sync::mpsc;

use crate::common::BridgeMessage;
use crate::config::types::GuildDashboardConfig;
use crate::discord::commands::WowCommand;
use crate::discord::resolver::MessageResolver;

/// Configuration for a channel mapping.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub discord_channel_id: Option<ChannelId>,
    pub discord_channel_name: String,
    pub wow_chat_type: u8,
    pub wow_channel_name: Option<String>,
    pub format_wow_to_discord: String,
    pub format_discord_to_wow: String,
}

/// Pending state before Discord channels are resolved.
///
/// This holds configuration loaded that needs Discord guild data
/// to resolve channel names to IDs.
#[derive(Debug)]
pub struct PendingBridgeState {
    /// Channel configs waiting for resolution: (channel_name, direction, config)
    pub pending_channel_configs: Vec<(String, String, ChannelConfig)>,
    /// Sender for messages going to WoW.
    pub wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Sender for commands going to WoW handler.
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
    /// Whether dot commands passthrough is enabled.
    pub enable_dot_commands: bool,
    /// Whitelist of allowed dot commands (None = all allowed if enabled).
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// Channels where commands are enabled (None = all channels).
    pub enable_commands_channels: Option<Vec<String>>,
    /// Whether to enable markdown (disable escaping).
    pub enable_markdown: bool,
    /// Whether to send tag resolution error notifications.
    pub enable_tag_failed_notifications: bool,
    /// Dashboard configuration.
    pub dashboard_config: Option<GuildDashboardConfig>,
}

impl PendingBridgeState {
    /// Create a new pending state.
    pub fn new(
        pending_channel_configs: Vec<(String, String, ChannelConfig)>,
        wow_tx: mpsc::UnboundedSender<BridgeMessage>,
        command_tx: mpsc::UnboundedSender<WowCommand>,
        enable_dot_commands: bool,
        dot_commands_whitelist: Option<Vec<String>>,
        enable_commands_channels: Option<Vec<String>>,
        enable_markdown: bool,
        enable_tag_failed_notifications: bool,
        dashboard_config: Option<GuildDashboardConfig>,
    ) -> Self {
        Self {
            pending_channel_configs,
            wow_tx,
            command_tx,
            enable_dot_commands,
            dot_commands_whitelist,
            enable_commands_channels,
            enable_markdown,
            enable_tag_failed_notifications,
            dashboard_config,
        }
    }

    /// Resolve Discord channels and build the final state.
    ///
    /// Returns `None` if critical channels couldn't be resolved.
    pub fn resolve(
        self,
        guild_channels: &[GuildChannel],
        http: Arc<Http>,
        self_user_id: u64,
    ) -> ResolvedBridgeState {
        let mut wow_to_discord: HashMap<(u8, Option<String>), Vec<ChannelConfig>> = HashMap::new();
        let mut discord_to_wow: HashMap<ChannelId, ChannelConfig> = HashMap::new();
        let mut resolved_channels: HashSet<ChannelId> = HashSet::new();
        let mut unresolved: Vec<(String, String, ChannelConfig)> = Vec::new();

        for (channel_name, direction, mut config) in self.pending_channel_configs {
            // Find matching Discord channel by name OR ID
            if let Some(discord_channel) = guild_channels.iter().find(|ch| {
                // First try matching as a channel ID (if the config value is numeric)
                if let Ok(channel_id) = channel_name.parse::<u64>() {
                    if ch.id.get() == channel_id {
                        return true;
                    }
                }
                // Then try matching by channel name (case-insensitive)
                ch.name().to_lowercase() == channel_name.to_lowercase()
            }) {
                // Update config with resolved channel ID
                config.discord_channel_id = Some(discord_channel.id);

                // Add to wow_to_discord mapping (lowercase channel name for consistent lookup)
                let key = (
                    config.wow_chat_type,
                    config.wow_channel_name.as_ref().map(|s| s.to_lowercase()),
                );
                wow_to_discord.entry(key).or_default().push(config.clone());

                // Add to discord_to_wow mapping if bidirectional
                if direction == "both" || direction == "discord_to_wow" {
                    discord_to_wow.insert(discord_channel.id, config.clone());
                }

                // Log resolution
                if resolved_channels.insert(discord_channel.id) {
                    let match_type = if channel_name.parse::<u64>().is_ok() {
                        "ID"
                    } else {
                        "name"
                    };
                    tracing::info!(
                        "Resolved Discord channel '{}' (by {}) -> #{} (ID {})",
                        channel_name,
                        match_type,
                        discord_channel.name(),
                        discord_channel.id
                    );
                } else {
                    tracing::debug!(
                        "Added additional mapping to '{}' for WoW channel {:?}",
                        channel_name,
                        config.wow_channel_name
                    );
                }
            } else {
                tracing::warn!("Could not resolve Discord channel: {}", channel_name);
                unresolved.push((channel_name, direction, config));
            }
        }

        // Resolve dashboard channel if config present
        let dashboard_channel_id = self.dashboard_config.as_ref().and_then(|config| {
            if !config.enabled {
                return None;
            }
            let channel_name = &config.channel;
            guild_channels
                .iter()
                .find(|ch| {
                    if let Ok(channel_id) = channel_name.parse::<u64>() {
                        if ch.id.get() == channel_id {
                            return true;
                        }
                    }
                    ch.name().to_lowercase() == channel_name.to_lowercase()
                })
                .map(|ch| {
                    tracing::info!(
                        "Resolved Dashboard channel '{}' -> #{} (ID {})",
                        channel_name,
                        ch.name(),
                        ch.id
                    );
                    ch.id
                })
        });

        if !unresolved.is_empty() {
            let remaining: Vec<&str> = unresolved
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect();
            tracing::warn!("Unresolved Discord channels: {:?}", remaining);
        }

        tracing::info!(
            "Channel resolution complete: {} resolved, {} unresolved",
            resolved_channels.len(),
            unresolved.len()
        );

        ResolvedBridgeState {
            wow_to_discord,
            discord_to_wow,
            wow_tx: self.wow_tx,
            command_tx: self.command_tx,
            resolver: MessageResolver::new(self.enable_markdown),
            enable_dot_commands: self.enable_dot_commands,
            dot_commands_whitelist: self.dot_commands_whitelist,
            enable_commands_channels: self.enable_commands_channels,
            http,
            self_user_id,
            enable_tag_failed_notifications: self.enable_tag_failed_notifications,
            dashboard_config: self.dashboard_config,
            dashboard_channel_id,
        }
    }
}

/// Fully resolved bridge state.
///
/// This is immutable after creation and can be safely shared via `Arc`
/// or cloned into task-specific contexts.
#[derive(Debug, Clone)]
pub struct ResolvedBridgeState {
    /// Map from (chat_type, channel_name) to Discord channels.
    pub wow_to_discord: HashMap<(u8, Option<String>), Vec<ChannelConfig>>,
    /// Map from Discord channel ID to WoW config.
    pub discord_to_wow: HashMap<ChannelId, ChannelConfig>,
    /// Sender for messages going to WoW.
    pub wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Sender for commands going to WoW handler.
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
    /// Message resolver.
    pub resolver: MessageResolver,
    /// Whether dot commands passthrough is enabled.
    pub enable_dot_commands: bool,
    /// Whitelist of allowed dot commands (None = all allowed if enabled).
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// Channels where commands are enabled (None = all channels).
    pub enable_commands_channels: Option<Vec<String>>,
    /// HTTP client for Discord API calls.
    pub http: Arc<Http>,
    /// Bot's user ID.
    pub self_user_id: u64,
    /// Whether to send tag resolution error notifications.
    pub enable_tag_failed_notifications: bool,
    /// Dashboard configuration.
    pub dashboard_config: Option<GuildDashboardConfig>,
    /// Resolved dashboard channel ID.
    pub dashboard_channel_id: Option<ChannelId>,
}

// ============================================================================
// Task-specific context structs
// ============================================================================

/// Context for the WoW -> Discord forwarding task.
///
/// Contains everything needed to process and forward messages from WoW to Discord.
#[derive(Clone)]
pub struct WowToDiscordContext {
    /// Map from (chat_type, channel_name) to Discord channels.
    pub wow_to_discord: HashMap<(u8, Option<String>), Vec<ChannelConfig>>,
    /// Message resolver for emoji/link/tag processing.
    pub resolver: MessageResolver,
    /// HTTP client for Discord API calls.
    pub http: Arc<Http>,
    /// Bot's user ID (for tag resolution).
    pub self_user_id: u64,
    /// Whether to send tag resolution error notifications.
    pub enable_tag_failed_notifications: bool,
    /// Sender for whisper replies (tag errors).
    pub wow_tx: mpsc::UnboundedSender<BridgeMessage>,
}

impl WowToDiscordContext {
    /// Create from resolved state.
    pub fn from_resolved(state: &ResolvedBridgeState) -> Self {
        Self {
            wow_to_discord: state.wow_to_discord.clone(),
            resolver: state.resolver.clone(),
            http: state.http.clone(),
            self_user_id: state.self_user_id,
            enable_tag_failed_notifications: state.enable_tag_failed_notifications,
            wow_tx: state.wow_tx.clone(),
        }
    }
}

/// Context for the Discord -> WoW message handling.
///
/// Contains everything needed to process incoming Discord messages.
#[derive(Clone)]
pub struct DiscordToWowContext {
    /// Map from Discord channel ID to WoW config.
    pub discord_to_wow: HashMap<ChannelId, ChannelConfig>,
    /// Message resolver for mention/emoji processing.
    pub resolver: MessageResolver,
    /// Sender for messages going to WoW.
    pub wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Whether dot commands passthrough is enabled.
    pub enable_dot_commands: bool,
    /// Whitelist of allowed dot commands.
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// Channels where commands are enabled.
    pub enable_commands_channels: Option<Vec<String>>,
}

impl DiscordToWowContext {
    /// Create from resolved state.
    pub fn from_resolved(state: &ResolvedBridgeState) -> Self {
        Self {
            discord_to_wow: state.discord_to_wow.clone(),
            resolver: state.resolver.clone(),
            wow_tx: state.wow_tx.clone(),
            enable_dot_commands: state.enable_dot_commands,
            dot_commands_whitelist: state.dot_commands_whitelist.clone(),
            enable_commands_channels: state.enable_commands_channels.clone(),
        }
    }

    /// Check if a dot command should be sent directly to WoW.
    pub fn should_send_dot_command_directly(&self, message: &str) -> bool {
        if message.len() > 100 || !self.enable_dot_commands || !message.starts_with('.') {
            return false;
        }

        let cmd = message[1..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();

        match &self.dot_commands_whitelist {
            None => true,
            Some(whitelist) => whitelist.iter().any(|allowed| {
                let allowed_lower = allowed.to_lowercase();
                cmd == allowed_lower
                    || (allowed_lower.ends_with('*')
                        && cmd.starts_with(&allowed_lower[..allowed_lower.len() - 1]))
            }),
        }
    }

    /// Check if commands are allowed in a given channel.
    pub fn command_allowed_in_channel(&self, channel_name: &str, channel_id: u64) -> bool {
        match &self.enable_commands_channels {
            Some(allowed) if !allowed.is_empty() => {
                let channel_lower = channel_name.to_lowercase();
                let channel_id_str = channel_id.to_string();
                allowed
                    .iter()
                    .any(|c| c.to_lowercase() == channel_lower || c == &channel_id_str)
            }
            _ => true,
        }
    }
}

/// Context for the command response forwarding task.
#[derive(Clone)]
pub struct CommandResponseContext {
    /// Message resolver for emoji processing.
    pub resolver: MessageResolver,
    /// HTTP client for Discord API calls.
    pub http: Arc<Http>,
    /// Bot's user ID.
    pub self_user_id: u64,
}

impl CommandResponseContext {
    /// Create from resolved state.
    pub fn from_resolved(state: &ResolvedBridgeState) -> Self {
        Self {
            resolver: state.resolver.clone(),
            http: state.http.clone(),
            self_user_id: state.self_user_id,
        }
    }
}

/// Context for the dashboard update task.
#[derive(Clone)]
pub struct DashboardContext {
    /// Resolved dashboard channel ID.
    pub channel_id: Option<ChannelId>,
    /// Dashboard configuration.
    pub config: GuildDashboardConfig,
}

impl DashboardContext {
    /// Create from resolved state.
    pub fn from_resolved(state: &ResolvedBridgeState, config: GuildDashboardConfig) -> Self {
        Self {
            channel_id: state.dashboard_channel_id,
            config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_resolved_state() -> ResolvedBridgeState {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        ResolvedBridgeState {
            wow_to_discord: HashMap::new(),
            discord_to_wow: HashMap::new(),
            wow_tx,
            command_tx: cmd_tx,
            resolver: MessageResolver::new(false),
            enable_dot_commands: true,
            dot_commands_whitelist: None,
            enable_commands_channels: None,
            http: Arc::new(Http::new("test")),
            self_user_id: 123456789,
            enable_tag_failed_notifications: false,
            dashboard_config: None,
            dashboard_channel_id: None,
        }
    }

    #[test]
    fn test_dot_command_passthrough_enabled() {
        let state = create_test_resolved_state();
        let ctx = DiscordToWowContext::from_resolved(&state);

        assert!(ctx.should_send_dot_command_directly(".help"));
        assert!(ctx.should_send_dot_command_directly(".guild info"));
        assert!(!ctx.should_send_dot_command_directly("hello world"));
        assert!(!ctx.should_send_dot_command_directly(""));
    }

    #[test]
    fn test_dot_command_whitelist() {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        let state = ResolvedBridgeState {
            wow_to_discord: HashMap::new(),
            discord_to_wow: HashMap::new(),
            wow_tx,
            command_tx: cmd_tx,
            resolver: MessageResolver::new(false),
            enable_dot_commands: true,
            dot_commands_whitelist: Some(vec!["help".to_string(), "guild*".to_string()]),
            enable_commands_channels: None,
            http: Arc::new(Http::new("test")),
            self_user_id: 123456789,
            enable_tag_failed_notifications: false,
            dashboard_config: None,
            dashboard_channel_id: None,
        };
        let ctx = DiscordToWowContext::from_resolved(&state);

        assert!(ctx.should_send_dot_command_directly(".help"));
        assert!(ctx.should_send_dot_command_directly(".guild"));
        assert!(ctx.should_send_dot_command_directly(".guildinfo"));
        assert!(!ctx.should_send_dot_command_directly(".who"));
        assert!(!ctx.should_send_dot_command_directly("hello"));
    }

    #[test]
    fn test_dot_command_disabled() {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        let state = ResolvedBridgeState {
            wow_to_discord: HashMap::new(),
            discord_to_wow: HashMap::new(),
            wow_tx,
            command_tx: cmd_tx,
            resolver: MessageResolver::new(false),
            enable_dot_commands: false,
            dot_commands_whitelist: None,
            enable_commands_channels: None,
            http: Arc::new(Http::new("test")),
            self_user_id: 123456789,
            enable_tag_failed_notifications: false,
            dashboard_config: None,
            dashboard_channel_id: None,
        };
        let ctx = DiscordToWowContext::from_resolved(&state);

        assert!(!ctx.should_send_dot_command_directly(".help"));
        assert!(!ctx.should_send_dot_command_directly(".anything"));
    }

    #[test]
    fn test_context_creation() {
        let state = create_test_resolved_state();

        let wow_ctx = WowToDiscordContext::from_resolved(&state);
        assert_eq!(wow_ctx.self_user_id, state.self_user_id);

        let discord_ctx = DiscordToWowContext::from_resolved(&state);
        assert_eq!(discord_ctx.enable_dot_commands, state.enable_dot_commands);

        let cmd_ctx = CommandResponseContext::from_resolved(&state);
        assert_eq!(cmd_ctx.self_user_id, state.self_user_id);
    }
}
