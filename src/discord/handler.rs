//! Discord message event handling.
//!
//! Provides the event handler for Discord messages and manages
//! the message flow between Discord and WoW.

use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::{GuildChannel, Message};
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::bridge::BridgeState;
use crate::common::{IncomingWowMessage, OutgoingWowMessage};
use crate::discord::commands::{CommandHandler, WowCommand};

// Re-export BridgeState's TypeMapKey implementation for Discord's context system
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
