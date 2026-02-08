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
use crate::config::types::GuildDashboardConfig;
use crate::common::messages::DashboardEvent;
use crate::discord::commands::{CommandHandler, CommandResponse, WowCommand};
use crate::discord::dashboard::DashboardRenderer;

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
    /// Receiver for command responses.
    cmd_response_rx: Arc<Mutex<mpsc::UnboundedReceiver<CommandResponse>>>,
    /// Receiver for status updates (from GameClient).
    status_rx: Arc<Mutex<mpsc::UnboundedReceiver<crate::common::ActivityStatus>>>,
    /// Receiver for dashboard updates.
    dashboard_rx: Arc<Mutex<mpsc::UnboundedReceiver<DashboardEvent>>>,
    /// Dashboard configuration.
    dashboard_config: GuildDashboardConfig,
    /// Shutdown signal.
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl BridgeHandler {
    pub fn new(
        wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
        command_tx: mpsc::UnboundedSender<WowCommand>,
        cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
        status_rx: mpsc::UnboundedReceiver<crate::common::ActivityStatus>,
        dashboard_rx: mpsc::UnboundedReceiver<DashboardEvent>,
        dashboard_config: GuildDashboardConfig,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            wow_rx: Arc::new(Mutex::new(wow_rx)),
            command_handler: CommandHandler::new(command_tx),
            cmd_response_rx: Arc::new(Mutex::new(cmd_response_rx)),
            status_rx: Arc::new(Mutex::new(status_rx)),
            dashboard_rx: Arc::new(Mutex::new(dashboard_rx)),
            dashboard_config,
            shutdown_rx,
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

        // Check for !commands first
        if content.len() <= 100 && (content.starts_with('!') || content.starts_with('?'))  {
            // Check if commands are enabled for this channel
            let should_handle = {
                let data = ctx.data.read().await;
                if let Some(state) = data.get::<BridgeState>() {
                    let state = state.read().await;
                    let channel_name = msg.channel_id.name(&ctx).await.unwrap_or_default();
                    state.command_allowed_in_channel(&channel_name, msg.channel_id.get())
                } else {
                    false
                }
            };
            if should_handle {
                match self.command_handler.handle_command(&ctx, &msg, content).await {
                    Ok(true) => return, // Command was handled
                    Ok(false) => {} // Not a known command, continue
                    Err(e) => {
                        error!("Command handler error: {}", e);
                        return;
                    }
                }
            }
            // If commands not enabled for this channel, fall through to regular message handling
        }

        if content.len() <= 100 && content.starts_with('.')  {
            let data = ctx.data.read().await;
            if let Some(state) = data.get::<BridgeState>() {
                let state = state.read().await;
                let channel_name = msg.channel_id.name(&ctx).await.unwrap_or_default();
                let should_send_directly = state.should_send_dot_command_directly(content)
                    && state.command_allowed_in_channel(&channel_name, msg.channel_id.get());
                if should_send_directly {
                    if let Some(bridge) = data.get::<Bridge>() {
                        let discord_msg = DiscordMessage {
                            sender: "".to_string(),
                            content: content.to_string(),
                            channel_id: msg.channel_id.get(),
                            channel_name: "".to_string(),
                        };
                        let outgoing = bridge.handle_discord_to_wow_directly(&discord_msg);
                        if let Some(msg) = outgoing {
                            if let Err(e) = state.wow_tx.send(msg) {
                                error!("Failed to send message to WoW: {}", e);
                            }
                        }
                    }
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

        // Start status update task
        let status_rx = Arc::clone(&self.status_rx);
        let status_ctx = ctx.clone();

        tokio::spawn(async move {
            let mut rx = status_rx.lock().await;

            while let Some(status) = rx.recv().await {
                use crate::common::ActivityStatus;
                use serenity::gateway::ActivityData;

                match status {
                    ActivityStatus::Connecting => {
                        let activity = ActivityData::custom("Connecting...");
                        status_ctx.set_activity(Some(activity));
                    },
                    ActivityStatus::Disconnected => {
                        let activity = ActivityData::custom("Offline");
                        status_ctx.set_activity(Some(activity));
                    },
                    ActivityStatus::ConnectedToRealm(realm) => {
                        let activity = ActivityData::custom(realm);
                        status_ctx.set_activity(Some(activity));
                    },
                    ActivityStatus::GuildStats { online_count } => {
                        let plural = if online_count == 1 { "" } else { "s" };
                        let text = if online_count == 0 {
                            "Currently no guildies online".to_string()
                        } else {
                            format!("{} guildie{} online", online_count, plural)
                        };
                        let activity = ActivityData::watching(text);
                        status_ctx.set_activity(Some(activity));
                    },
                }
            }

            info!("Status update task ended");
        });

        // Start dashboard update task
        let dashboard_rx = Arc::clone(&self.dashboard_rx);
        let dashboard_ctx = ctx.clone();
        let dashboard_config = self.dashboard_config.clone();
        let dashboard_data_ref = ctx.data.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut rx = dashboard_rx.lock().await;
            let mut dashboard = DashboardRenderer::new(dashboard_config);

            loop {
                tokio::select! {
                    event_opt = rx.recv() => {
                        match event_opt {
                            Some(event) => {
                                // Check shutdown first
                                if *shutdown_rx.borrow() {
                                    break;
                                }

                                // Get resolved channel ID from BridgeState
                                let channel_id = {
                                    let data = dashboard_data_ref.read().await;
                                    if let Some(state) = data.get::<BridgeState>() {
                                        let state = state.read().await;
                                        state.dashboard_channel_id
                                    } else {
                                        None
                                    }
                                };

                                match event {
                                    DashboardEvent::Update(data) => {
                                        dashboard.update(&dashboard_ctx, channel_id, data).await;
                                    },
                                    DashboardEvent::SetOffline => {
                                        dashboard.set_offline(&dashboard_ctx, channel_id).await;
                                    }
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                }
            }

            info!("Dashboard update task ended");
        });


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
                            msg.guild_event.as_ref(),
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

        // Start command response forwarding task
        let cmd_response_rx = Arc::clone(&self.cmd_response_rx);
        let cmd_http = ctx.http.clone();
        let cmd_data = ctx.data.clone();
        let mut shutdown_rx_cmd = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut rx = cmd_response_rx.lock().await;

            loop {
                tokio::select! {
                    response_opt = rx.recv() => {
                        match response_opt {
                            Some(response) => {
                                // Check shutdown first
                                if *shutdown_rx_cmd.borrow() {
                                    break;
                                }

                                let data = cmd_data.read().await;
                                if let Some(bridge) = data.get::<Bridge>() {
                                    if let Some(state) = data.get::<BridgeState>() {
                                        let state = state.read().await;

                                        // Apply bridge formatting (e.g., MOTD format)
                                        let formatted = bridge.format_command_response(&response.content);

                                        // Apply post-bridge processing (emojis, markdown escape)
                                        let result = state.resolver.process_post_bridge(
                                            &ctx.cache,
                                            serenity::model::id::ChannelId::new(response.channel_id),
                                            &formatted,
                                            state.self_user_id.unwrap_or(0),
                                        );

                                        // Send to Discord
                                        use serenity::model::id::ChannelId;
                                        let channel = ChannelId::new(response.channel_id);
                                        if let Err(e) = channel.say(&cmd_http, &result.message).await {
                                            error!("Failed to send command response to Discord: {}", e);
                                        }
                                    }
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    _ = shutdown_rx_cmd.changed() => {
                        if *shutdown_rx_cmd.borrow() {
                            break;
                        }
                    }
                }
            }
            info!("Command response forwarding task ended");
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
