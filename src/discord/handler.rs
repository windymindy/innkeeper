//! Discord message event handling.
//!
//! Provides the event handler for Discord messages and manages
//! the message flow between Discord and WoW.

use std::collections::HashMap;
use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::discord::commands::{CommandHandler, WowCommand};
use crate::discord::resolver::MessageResolver;

/// Configuration for a channel mapping.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub discord_channel_id: ChannelId,
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
    /// Bot's own user ID (set on ready).
    pub self_id: Option<serenity::model::id::UserId>,
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
        {
            let data = ctx.data.read().await;
            if let Some(state) = data.get::<BridgeState>() {
                let state = state.read().await;
                if let Some(self_id) = state.self_id {
                    if msg.author.id == self_id {
                        return;
                    }
                }
            }
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

                // Check for dot commands passthrough
                let is_dot_command = processed.starts_with('.');

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

                debug!(
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

        // Store our user ID
        {
            let data = ctx.data.read().await;
            if let Some(state) = data.get::<BridgeState>() {
                let mut state = state.write().await;
                state.self_id = Some(ready.user.id);
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
                            match config.discord_channel_id.say(&http, &formatted).await {
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
