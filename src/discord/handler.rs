//! Discord message event handling.
//!
//! Provides the event handler for Discord messages and manages
//! the message flow between Discord and WoW.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serenity::async_trait;
use serenity::http::Http;
use serenity::model::channel::{GuildChannel, Message};
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::discord::commands::{CommandHandler, WowCommand};
use crate::discord::resolver::MessageResolver;
use crate::game::filter::MessageFilter;

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

/// Message from Discord destined for WoW.
#[derive(Debug, Clone)]
pub struct OutgoingWowMessage {
    pub chat_type: u8,
    pub channel_name: Option<String>,
    pub sender: String,
    pub content: String,
}

/// Message from WoW destined for Discord.
#[derive(Debug, Clone)]
pub struct IncomingWowMessage {
    pub sender: Option<String>,
    pub content: String,
    pub chat_type: u8,
    pub channel_name: Option<String>,
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
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
    /// Message resolver.
    pub resolver: MessageResolver,
    /// Message filter for filtering spam/unwanted messages.
    pub filter: MessageFilter,
    /// Pending channel configs waiting for Discord channel ID resolution.
    /// Stored as (channel_name, direction, config) tuples.
    pub pending_channel_configs: Vec<(String, String, ChannelConfig)>,
    /// Whether dot commands passthrough is enabled.
    pub enable_dot_commands: bool,
    /// Whitelist of allowed dot commands (None = all allowed if enabled).
    pub dot_commands_whitelist: Option<Vec<String>>,
    /// HTTP client for Discord API calls.
    pub http: Option<Arc<Http>>,
}

impl BridgeState {
    /// Resolve Discord channel IDs from channel names after bot connects.
    /// Returns the number of channels successfully resolved.
    pub fn resolve_discord_channels(&mut self, guild_channels: &[GuildChannel]) -> usize {
        let mut resolved_count = 0;
        let mut unresolved = Vec::new();
        let mut resolved_channels: HashSet<ChannelId> = HashSet::new();

        // Clear existing mappings to rebuild them
        self.wow_to_discord.clear();
        self.discord_to_wow.clear();

        // Take ownership of pending configs
        let pending = std::mem::take(&mut self.pending_channel_configs);

        for (channel_name, direction, mut config) in pending {
            // Find matching Discord channel by name
            if let Some(discord_channel) = guild_channels.iter().find(|ch| {
                ch.name().to_lowercase() == channel_name.to_lowercase()
            }) {
                // Update config with resolved channel ID
                config.discord_channel_id = Some(discord_channel.id);

                // Add to wow_to_discord mapping (lowercase channel name for consistent lookup)
                let key = (config.wow_chat_type, config.wow_channel_name.as_ref().map(|s| s.to_lowercase()));
                self.wow_to_discord.entry(key).or_default().push(config.clone());

                // Add to discord_to_wow mapping if bidirectional
                if direction == "both" || direction == "discord_to_wow" {
                    self.discord_to_wow.insert(discord_channel.id, config.clone());
                }

                // Only log "Resolved" for the first time we see this Discord channel
                if resolved_channels.insert(discord_channel.id) {
                    info!("Resolved Discord channel '{}' -> ID {}", channel_name, discord_channel.id);
                } else {
                    debug!("Added additional mapping to '{}' for WoW channel {:?}", channel_name, config.wow_channel_name);
                }
                
                resolved_count += 1;
            } else {
                warn!("Could not resolve Discord channel: {}", channel_name);
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
        let cmd = message[1..].split_whitespace().next().unwrap_or("").to_lowercase();

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
                if allowed_lower.ends_with('*') && cmd.starts_with(&allowed_lower[..allowed_lower.len()-1]) {
                    return true;
                }
            }
        }

        false
    }
}

impl TypeMapKey for BridgeState {
    type Value = Arc<RwLock<BridgeState>>;
}

/// Discord event handler.
pub struct BridgeHandler {
    /// Receiver for WoW -> Discord messages.
    wow_rx: Arc<Mutex<mpsc::UnboundedReceiver<IncomingWowMessage>>>,
    /// Command handler.
    command_handler: CommandHandler,
}

impl BridgeHandler {
    pub fn new(
        wow_rx: mpsc::UnboundedReceiver<IncomingWowMessage>,
        command_tx: mpsc::UnboundedSender<WowCommand>,
    ) -> Self {
        Self {
            wow_rx: Arc::new(Mutex::new(wow_rx)),
            command_handler: CommandHandler::new(command_tx),
        }
    }
}

#[async_trait]
impl EventHandler for BridgeHandler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore our own messages
        if msg.author.id == ctx.cache.current_user().id {
            return;
        }

        // Ignore bots
        if msg.author.bot {
            return;
        }

        // Only handle guild (server) messages
        if msg.guild_id.is_none() {
            return;
        }

        let content = msg.content.trim();
        if content.is_empty() && msg.attachments.is_empty() {
            return;
        }

        // Check for commands first
        if content.starts_with('!') {
            match self.command_handler.handle_command(&ctx, &msg, content).await {
                Ok(true) => return, // Command was handled
                Ok(false) => {} // Not a known command, continue
                Err(e) => {
                    error!("Command handler error: {}", e);
                    return;
                }
            }
        }

        // Process as a regular message
        let data = ctx.data.read().await;
        if let Some(state) = data.get::<BridgeState>() {
            let state = state.read().await;

            let channel_id = msg.channel_id;
            if let Some(config) = state.discord_to_wow.get(&channel_id) {
                // Get effective display name
                let sender = msg
                    .member
                    .as_ref()
                    .and_then(|m| m.nick.clone())
                    .unwrap_or_else(|| msg.author.name.clone());

                // Build message content including attachments
                let mut full_content = content.to_string();
                for attachment in &msg.attachments {
                    if !full_content.is_empty() {
                        full_content.push(' ');
                    }
                    full_content.push_str(&attachment.url);
                }

                // Process the message for WoW
                let processed = state.resolver.process_discord_to_wow(&full_content, &ctx.cache);

                // Check if message should be filtered
                if state.filter.should_filter_discord_to_wow(&processed) {
                    info!("FILTERED Discord -> WoW: {}", processed);
                    return;
                }

                // Check for dot commands passthrough
                let is_dot_command = state.should_send_dot_command_directly(&processed);

                // Format the message
                let formatted = if is_dot_command {
                    // Send dot commands as-is
                    processed
                } else {
                    config
                        .format_discord_to_wow
                        .replace("%user", &sender)
                        .replace("%message", &processed)
                };

                info!(
                    "Discord -> WoW [{}]: {}",
                    config.wow_channel_name.as_deref().unwrap_or("guild"),
                    formatted
                );

                let outgoing = OutgoingWowMessage {
                    chat_type: config.wow_chat_type,
                    channel_name: config.wow_channel_name.clone(),
                    sender,
                    content: formatted,
                };

                if let Err(e) = state.wow_tx.send(outgoing) {
                    error!("Failed to send message to WoW: {}", e);
                }
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        // Store our user ID and HTTP client, then wait for guild data
        {
            let data = ctx.data.read().await;
            if let Some(state) = data.get::<BridgeState>() {
                let mut state = state.write().await;
                state.http = Some(ctx.http.clone());

                info!("Connected to Discord. Waiting for guild data to resolve channels...");
                info!("Pending channels to resolve: {}", state.pending_channel_configs.len());
            }
        }

        // Start WoW -> Discord forwarding task
        let wow_rx = Arc::clone(&self.wow_rx);
        let http = ctx.http.clone();
        let cache = ctx.cache.clone();
        let data = ctx.data.clone();

        tokio::spawn(async move {
            let mut rx = wow_rx.lock().await;

            while let Some(msg) = rx.recv().await {
                let data = data.read().await;
                if let Some(state) = data.get::<BridgeState>() {
                    let state = state.read().await;

                    let key = (
                        msg.chat_type,
                        msg.channel_name.as_ref().map(|s| s.to_lowercase()),
                    );

                    if let Some(configs) = state.wow_to_discord.get(&key) {
                        for config in configs {
                            // Process the message
                            let processed = state.resolver.process_wow_to_discord(&cache, &msg.content);

                            // Format the message
                            let formatted = config
                                .format_wow_to_discord
                                .replace("%user", msg.sender.as_deref().unwrap_or(""))
                                .replace("%message", &processed)
                                .replace("%target", msg.channel_name.as_deref().unwrap_or(""));

                            // Send to Discord
                            if let Some(channel_id) = config.discord_channel_id {
                                match channel_id.say(&http, &formatted).await {
                                    Ok(_) => {
                                        info!(
                                            "WoW -> Discord [{}]: {}",
                                            config.discord_channel_name, formatted
                                        );
                                    }
                                    Err(e) => {
                                        error!(
                                            "Failed to send to Discord channel {}: {}",
                                            config.discord_channel_name, e
                                        );
                                    }
                                }
                            } else {
                                warn!("Discord channel ID not resolved for {}", config.discord_channel_name);
                            }
                        }
                    } else {
                        debug!(
                            "No Discord channel mapping for WoW chat type {} channel {:?}",
                            msg.chat_type, msg.channel_name
                        );
                    }
                }
            }

            warn!("WoW -> Discord forwarding task ended");
        });
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        info!("Received guild data for '{}' ({} channels)", guild.name, guild.channels.len());

        let data = ctx.data.read().await;
        if let Some(state) = data.get::<BridgeState>() {
            let mut state = state.write().await;

            // Get channel names for logging
            let _channel_names: Vec<String> = guild.channels.values()
                .map(|ch| ch.name.clone())
                .collect();

            // Convert HashMap values to Vec for resolution
            let guild_channels: Vec<GuildChannel> = guild.channels.values().cloned().collect();

            // Log what channels we're looking for
            let pending_names: Vec<String> = state.pending_channel_configs.iter()
                .map(|(name, _, _)| name.clone())
                .collect();

            let resolved = state.resolve_discord_channels(&guild_channels);
            info!("Resolved {} Discord channels from guild '{}'", resolved, guild.name);

            if resolved > 0 {
                info!("Successfully resolved channels: {:?}", 
                    guild_channels.iter()
                        .filter(|ch| pending_names.contains(&ch.name))
                        .map(|ch| format!("{} -> {}", ch.name, ch.id))
                        .collect::<Vec<_>>()
                );
            }

            // Log remaining unresolved
            if !state.pending_channel_configs.is_empty() {
                let remaining: Vec<String> = state.pending_channel_configs.iter()
                    .map(|(name, _, _)| name.clone())
                    .collect();
                info!("Still waiting to resolve channels: {:?}", remaining);
            } else {
                info!("All Discord channels resolved successfully");
            }
        }
    }
}

/// Create channels for the bridge communication.
pub fn create_bridge_channels() -> (
    mpsc::UnboundedSender<OutgoingWowMessage>,
    mpsc::UnboundedReceiver<OutgoingWowMessage>,
    mpsc::UnboundedSender<IncomingWowMessage>,
    mpsc::UnboundedReceiver<IncomingWowMessage>,
    mpsc::UnboundedSender<WowCommand>,
    mpsc::UnboundedReceiver<WowCommand>,
) {
    let (wow_tx, wow_rx) = mpsc::unbounded_channel();
    let (discord_tx, discord_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();

    (wow_tx, wow_rx, discord_tx, discord_rx, command_tx, command_rx)
}
