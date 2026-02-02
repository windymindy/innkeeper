//! Bridge state management.
//!
//! Provides shared state for coordinating Discord-WoW message flow,
//! including channel mappings and configuration.

use std::collections::{HashMap, HashSet};

use serenity::http::Http;
use serenity::model::channel::GuildChannel;
use serenity::model::id::ChannelId;
use tokio::sync::mpsc;

use crate::common::OutgoingWowMessage;
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

/// Shared state accessible from the event handler.
pub struct BridgeState {
    /// Map from (chat_type, channel_name) to Discord channels.
    pub wow_to_discord: HashMap<(u8, Option<String>), Vec<ChannelConfig>>,
    /// Map from Discord channel ID to WoW config.
    pub discord_to_wow: HashMap<ChannelId, ChannelConfig>,
    /// Sender for messages going to WoW.
    pub wow_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
    /// Sender for commands going to WoW handler.
    pub command_tx: mpsc::UnboundedSender<crate::discord::commands::WowCommand>,
    /// Message resolver.
    pub resolver: MessageResolver,
    /// Pending channel configs waiting for Discord channel ID resolution.
    /// Stored as (channel_name, direction, config) tuples.
    pub pending_channel_configs: Vec<(String, String, ChannelConfig)>,
    /// Whether dot commands passthrough is enabled.
    pub enable_dot_commands: bool,
    /// Whitelist of allowed dot commands (None = all allowed if enabled).
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// HTTP client for Discord API calls.
    pub http: Option<std::sync::Arc<Http>>,
}

impl BridgeState {
    /// Create a new bridge state.
    pub fn new(
        wow_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
        command_tx: mpsc::UnboundedSender<crate::discord::commands::WowCommand>,
        enable_dot_commands: bool,
        dot_commands_whitelist: Option<Vec<String>>,
    ) -> Self {
        Self {
            wow_to_discord: HashMap::new(),
            discord_to_wow: HashMap::new(),
            wow_tx,
            command_tx,
            resolver: MessageResolver::new(),
            pending_channel_configs: Vec::new(),
            enable_dot_commands,
            dot_commands_whitelist,
            http: None,
        }
    }

    /// Resolve Discord channel IDs from channel names after bot connects.
    /// Returns the number of channels successfully resolved.
    pub fn resolve_discord_channels(&mut self, guild_channels: &[GuildChannel]) -> usize {
        let mut unresolved = Vec::new();
        let mut resolved_channels: HashSet<ChannelId> = HashSet::new();

        // Clear existing mappings to rebuild them
        self.wow_to_discord.clear();
        self.discord_to_wow.clear();

        // Take ownership of pending configs
        let pending = std::mem::take(&mut self.pending_channel_configs);

        for (channel_name, direction, mut config) in pending {
            // Find matching Discord channel by name
            if let Some(discord_channel) = guild_channels
                .iter()
                .find(|ch| ch.name().to_lowercase() == channel_name.to_lowercase())
            {
                // Update config with resolved channel ID
                config.discord_channel_id = Some(discord_channel.id);

                // Add to wow_to_discord mapping (lowercase channel name for consistent lookup)
                let key = (
                    config.wow_chat_type,
                    config.wow_channel_name.as_ref().map(|s| s.to_lowercase()),
                );
                self.wow_to_discord
                    .entry(key)
                    .or_default()
                    .push(config.clone());

                // Add to discord_to_wow mapping if bidirectional
                if direction == "both" || direction == "discord_to_wow" {
                    self.discord_to_wow
                        .insert(discord_channel.id, config.clone());
                }

                // Only log "Resolved" for the first time we see this Discord channel
                if resolved_channels.insert(discord_channel.id) {
                    tracing::info!(
                        "Resolved Discord channel '{}' -> ID {}",
                        channel_name,
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
                // Save for retry later
                unresolved.push((channel_name, direction, config));
            }
        }

        // Put back unresolved configs
        self.pending_channel_configs = unresolved;

        // Return the number of unique Discord channels resolved
        resolved_channels.len()
    }

    /// Check if a dot command message should be sent directly to WoW (passthrough).
    /// Returns true if the command is allowed based on whitelist settings.
    pub fn should_send_dot_command_directly(&self, message: &str) -> bool {
        if !self.enable_dot_commands || !message.starts_with('.') {
            return false;
        }

        // Extract the command name (everything after '.' until first space)
        let cmd = message[1..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();

        // If no whitelist, all dot commands are allowed
        if self.dot_commands_whitelist.is_none() {
            return true;
        }

        // Check against whitelist
        if let Some(ref whitelist) = self.dot_commands_whitelist {
            for allowed in whitelist {
                let allowed_lower = allowed.to_lowercase();
                // Check exact match
                if cmd == allowed_lower {
                    return true;
                }
                // Check prefix match for wildcard patterns (e.g., "guild*" matches "guildinfo")
                if allowed_lower.ends_with('*')
                    && cmd.starts_with(&allowed_lower[..allowed_lower.len() - 1])
                {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> BridgeState {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        BridgeState::new(wow_tx, cmd_tx, true, None)
    }

    #[test]
    fn test_dot_command_passthrough_enabled() {
        let state = create_test_state();

        assert!(state.should_send_dot_command_directly(".help"));
        assert!(state.should_send_dot_command_directly(".guild info"));
        assert!(!state.should_send_dot_command_directly("hello world"));
        assert!(!state.should_send_dot_command_directly(""));
    }

    #[test]
    fn test_dot_command_whitelist() {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        let whitelist = Some(vec!["help".to_string(), "guild*".to_string()]);
        let state = BridgeState::new(wow_tx, cmd_tx, true, whitelist);

        assert!(state.should_send_dot_command_directly(".help"));
        assert!(state.should_send_dot_command_directly(".guild"));
        assert!(state.should_send_dot_command_directly(".guildinfo"));
        assert!(!state.should_send_dot_command_directly(".who"));
        assert!(!state.should_send_dot_command_directly("hello"));
    }

    #[test]
    fn test_dot_command_disabled() {
        let (wow_tx, _) = mpsc::unbounded_channel();
        let (cmd_tx, _) = mpsc::unbounded_channel();

        let state = BridgeState::new(wow_tx, cmd_tx, false, None);

        assert!(!state.should_send_dot_command_directly(".help"));
        assert!(!state.should_send_dot_command_directly(".anything"));
    }
}
