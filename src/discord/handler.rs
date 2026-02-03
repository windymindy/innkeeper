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

use crate::bridge::{Bridge, BridgeState};
use crate::common::{BridgeMessage, DiscordMessage};
use crate::discord::commands::{CommandHandler, WowCommand};

// Re-export BridgeState's TypeMapKey implementation for Discord's context system
impl TypeMapKey for BridgeState {
    type Value = Arc<RwLock<BridgeState>>;
}

// Store Bridge in context for access by handler
impl TypeMapKey for Bridge {
    type Value = Arc<Bridge>;
}

/// Discord event handler.
pub struct BridgeHandler {
    /// Receiver for WoW -> Discord messages.
    wow_rx: Arc<Mutex<mpsc::UnboundedReceiver<BridgeMessage>>>,
    /// Command handler.
    command_handler: CommandHandler,
}

impl BridgeHandler {
    pub fn new(
        wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
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

        // Process as a regular message using the Bridge for filtering and routing
        let data = ctx.data.read().await;
        if let Some(bridge) = data.get::<Bridge>() {
            if let Some(state) = data.get::<BridgeState>() {
                let state = state.read().await;

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

                // Process the message (resolve emojis, mentions, etc.)
                let processed = state.resolver.process_discord_to_wow(&full_content, &ctx.cache);

                // Create DiscordMessage and use Bridge to process, filter, and format
                let discord_msg = DiscordMessage {
                    sender: sender.clone(),
                    content: processed,
                    channel_id: msg.channel_id.get(),
                    channel_name: state
                        .discord_to_wow
                        .get(&msg.channel_id)
                        .map(|c| c.discord_channel_name.clone())
                        .unwrap_or_default(),
                };

                let outgoing = bridge.handle_discord_to_wow(&discord_msg);
                for msg in outgoing {
                    if let Err(e) = state.wow_tx.send(msg) {
                        error!("Failed to send message to WoW: {}", e);
                    }
                }
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        // Store our user ID and HTTP client, then wait for guild data
        let self_user_id = ready.user.id.get();
        {
            let data = ctx.data.read().await;
            if let Some(state) = data.get::<BridgeState>() {
                let mut state = state.write().await;
                state.http = Some(ctx.http.clone());
                state.self_user_id = Some(self_user_id);

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
                if let Some(bridge) = data.get::<Bridge>() {
                    if let Some(state) = data.get::<BridgeState>() {
                        let state = state.read().await;
                        let self_user_id = state.self_user_id.unwrap_or(0);
                        let enable_tag_notifications = state.enable_tag_failed_notifications;

                        // Pre-process WoW content (resolve links, strip colors/textures) BEFORE formatting
                        let processed_content = state.resolver.process_pre_bridge(&msg.content);

                        // Process and filter message through Bridge (with optional format override)
                        let results = bridge.handle_wow_to_discord(
                            msg.chat_type,
                            msg.channel_name.as_deref(),
                            msg.sender.as_deref(),
                            &processed_content,
                            msg.format.as_deref(),
                            msg.guild_event.as_deref(),
                        );

                        // Send filtered messages to appropriate Discord channels
                        for (discord_channel_name, formatted) in results {
                            // Find the Discord channel ID from state
                            if let Some(channel_configs) = state.wow_to_discord.get(&(
                                msg.chat_type,
                                msg.channel_name.as_ref().map(|s| s.to_lowercase()),
                            )) {
                                for config in channel_configs {
                                    if config.discord_channel_name == discord_channel_name {
                                        if let Some(channel_id) = config.discord_channel_id {
                                        // Apply post-bridge processing (emojis, tags, markdown escape)
                                        let (final_message, tag_errors) = if msg.sender.is_some() {
                                            let result = state.resolver.process_post_bridge(
                                                &cache,
                                                channel_id,
                                                &formatted,
                                                self_user_id,
                                            );
                                            (result.message, result.errors)
                                        } else {
                                            (formatted.clone(), Vec::new())
                                        };

                                            // Send the message to Discord
                                            match channel_id.say(&http, &final_message).await {
                                                Ok(_) => {
                                                    debug!(
                                                        "Sent to Discord #{}: {}",
                                                        discord_channel_name, final_message
                                                    );
                                                }
                                                Err(e) => {
                                                    error!("Failed to send to Discord channel {}: {}", discord_channel_name, e);
                                                }
                                            }

                                            // Handle tag resolution errors
                                            if enable_tag_notifications && !tag_errors.is_empty() {
                                                for error_msg in &tag_errors {
                                                    // Send error to Discord channel
                                                    if let Err(e) = channel_id.say(&http, error_msg).await {
                                                        warn!("Failed to send tag error to Discord: {}", e);
                                                    }

                                                    // Send whisper back to WoW sender
                                                    if let Some(ref sender) = msg.sender {
                                                        let whisper_msg = BridgeMessage {
                                                            sender: None,
                                                            content: error_msg.clone(),
                                                            chat_type: 7, // CHAT_MSG_WHISPER
                                                            channel_name: Some(sender.clone()),
                                                            format: None,
                                                            guild_event: None,
                                                        };
                                                        if let Err(e) = state.wow_tx.send(whisper_msg) {
                                                            warn!("Failed to send tag error whisper to WoW: {}", e);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            info!("WoW -> Discord forwarding task ended");
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
