//! Discord message event handling.
//!
//! Provides the event handler for Discord messages and manages
//! the message flow between Discord and WoW.
//!
//! ## Initialization Flow
//!
//! 1. `ready()`: Stores HTTP client and user ID, signals ready state
//! 2. `guild_create()`: Resolves channels, builds ResolvedBridgeState, spawns tasks
//! 3. Background tasks run with owned copies of their required state

use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::prelude::*;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, warn};

use crate::bridge::{
    Bridge, CommandResponseContext, DashboardContext, DiscordToWowContext, PendingBridgeState,
    ResolvedBridgeState, WowToDiscordContext,
};
use crate::common::messages::DashboardEvent;
use crate::common::{ActivityStatus, BridgeMessage, DiscordMessage};
use crate::config::types::GuildDashboardConfig;
use crate::discord::commands::{CommandHandler, CommandResponse, WowCommand};
use crate::discord::dashboard::DashboardRenderer;

/// Data passed from ready() to guild_create() for state resolution.
struct ReadyData {
    self_user_id: u64,
}

/// Channels bundle for background tasks.
/// These are consumed when tasks are spawned (moved into the tasks).
pub struct TaskChannels {
    pub wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    pub cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
    pub status_rx: mpsc::UnboundedReceiver<ActivityStatus>,
    pub dashboard_rx: mpsc::UnboundedReceiver<DashboardEvent>,
}

/// Discord event handler.
///
/// Manages the bridge lifecycle:
/// - Receives Discord events from serenity
/// - Coordinates state resolution between ready() and guild_create()
/// - Spawns background tasks after state is fully resolved
pub struct BridgeHandler {
    /// Bridge for message routing/formatting (immutable, shared).
    bridge: Arc<Bridge>,
    /// Pending state waiting for resolution (consumed in guild_create).
    pending_state: std::sync::Mutex<Option<PendingBridgeState>>,
    /// Task channels waiting to be consumed (consumed in guild_create).
    task_channels: std::sync::Mutex<Option<TaskChannels>>,
    /// Ready data sender (used once in ready()).
    ready_tx: std::sync::Mutex<Option<oneshot::Sender<ReadyData>>>,
    /// Ready data receiver (used once in guild_create()).
    ready_rx: std::sync::Mutex<Option<oneshot::Receiver<ReadyData>>>,
    /// Command handler for Discord commands.
    command_handler: CommandHandler,
    /// Dashboard configuration.
    dashboard_config: GuildDashboardConfig,
    /// Shutdown signal receiver.
    shutdown_rx: watch::Receiver<bool>,
    /// Resolved state (available after guild_create, for message() handler).
    resolved_state: std::sync::Mutex<Option<Arc<ResolvedBridgeState>>>,
}

impl BridgeHandler {
    /// Create a new bridge handler.
    ///
    /// The handler takes ownership of all channels and pending state.
    /// Tasks are NOT spawned until guild_create() resolves the state.
    pub fn new(
        bridge: Arc<Bridge>,
        pending_state: PendingBridgeState,
        task_channels: TaskChannels,
        command_tx: mpsc::UnboundedSender<WowCommand>,
        dashboard_config: GuildDashboardConfig,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        let (ready_tx, ready_rx) = oneshot::channel();

        Self {
            bridge,
            pending_state: std::sync::Mutex::new(Some(pending_state)),
            task_channels: std::sync::Mutex::new(Some(task_channels)),
            ready_tx: std::sync::Mutex::new(Some(ready_tx)),
            ready_rx: std::sync::Mutex::new(Some(ready_rx)),
            command_handler: CommandHandler::new(command_tx),
            dashboard_config,
            shutdown_rx,
            resolved_state: std::sync::Mutex::new(None),
        }
    }

    /// Spawn background tasks with owned copies of their required context.
    fn spawn_background_tasks(
        &self,
        ctx: &Context,
        resolved_state: &ResolvedBridgeState,
        mut task_channels: TaskChannels,
    ) {
        // Create task-specific contexts (owned copies)
        let wow_to_discord_ctx = WowToDiscordContext::from_resolved(resolved_state);
        let cmd_response_ctx = CommandResponseContext::from_resolved(resolved_state);
        let dashboard_ctx =
            DashboardContext::from_resolved(resolved_state, self.dashboard_config.clone());

        // Spawn status update task
        let status_ctx = ctx.clone();
        let mut status_rx = task_channels.status_rx;
        tokio::spawn(async move {
            while let Some(status) = status_rx.recv().await {
                use serenity::gateway::ActivityData;

                match status {
                    ActivityStatus::Connecting => {
                        let activity = ActivityData::custom("Connecting...");
                        status_ctx.set_activity(Some(activity));
                    }
                    ActivityStatus::Disconnected => {
                        let activity = ActivityData::custom("Offline");
                        status_ctx.set_activity(Some(activity));
                    }
                    ActivityStatus::ConnectedToRealm(realm) => {
                        let activity = ActivityData::custom(realm);
                        status_ctx.set_activity(Some(activity));
                    }
                    ActivityStatus::GuildStats { online_count } => {
                        let plural = if online_count == 1 { "" } else { "s" };
                        let text = if online_count == 0 {
                            "Currently no guildies online".to_string()
                        } else {
                            format!("{} guildie{} online", online_count, plural)
                        };
                        let activity = ActivityData::watching(text);
                        status_ctx.set_activity(Some(activity));
                    }
                }
            }
            info!("Status update task ended");
        });

        // Spawn dashboard update task
        let dashboard_ctx_serenity = ctx.clone();
        let mut dashboard_rx = task_channels.dashboard_rx;
        let mut dashboard_shutdown = self.shutdown_rx.clone();
        tokio::spawn(async move {
            let mut dashboard = DashboardRenderer::new(dashboard_ctx.config);

            loop {
                tokio::select! {
                    event_opt = dashboard_rx.recv() => {
                        match event_opt {
                            Some(event) => {
                                if *dashboard_shutdown.borrow() {
                                    break;
                                }
                                match event {
                                    DashboardEvent::Update(data) => {
                                        dashboard.update(&dashboard_ctx_serenity, dashboard_ctx.channel_id, data).await;
                                    }
                                    DashboardEvent::SetOffline => {
                                        dashboard.set_offline(&dashboard_ctx_serenity, dashboard_ctx.channel_id).await;
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                    _ = dashboard_shutdown.changed() => {
                        if *dashboard_shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
            info!("Dashboard update task ended");
        });

        // Spawn WoW -> Discord forwarding task
        let bridge = self.bridge.clone();
        let cache = ctx.cache.clone();
        let mut wow_rx = task_channels.wow_rx;
        tokio::spawn(async move {
            while let Some(msg) = wow_rx.recv().await {
                // Pre-process WoW content (resolve links, strip colors/textures)
                let processed_content = wow_to_discord_ctx.resolver.process_pre_bridge(&msg.content);

                // Process and filter message through Bridge
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
                    let key = (
                        msg.chat_type,
                        msg.channel_name.as_ref().map(|s| s.to_lowercase()),
                    );
                    if let Some(channel_configs) = wow_to_discord_ctx.wow_to_discord.get(&key) {
                        for config in channel_configs {
                            if config.discord_channel_name == discord_channel_name {
                                if let Some(channel_id) = config.discord_channel_id {
                                    // Apply post-bridge processing (emojis, tags, markdown escape)
                                    let (final_message, tag_errors) = if msg.sender.is_some() {
                                        let result = wow_to_discord_ctx.resolver.process_post_bridge(
                                            &cache,
                                            channel_id,
                                            &formatted,
                                            wow_to_discord_ctx.self_user_id,
                                        );
                                        (result.message, result.errors)
                                    } else {
                                        (formatted.clone(), Vec::new())
                                    };

                                    // Send the message to Discord
                                    match channel_id.say(&wow_to_discord_ctx.http, &final_message).await {
                                        Ok(_) => {
                                            debug!(
                                                "Sent to Discord #{}: {}",
                                                discord_channel_name, final_message
                                            );
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to send to Discord channel {}: {}",
                                                discord_channel_name, e
                                            );
                                        }
                                    }

                                    // Handle tag resolution errors
                                    if wow_to_discord_ctx.enable_tag_failed_notifications
                                        && !tag_errors.is_empty()
                                    {
                                        for error_msg in &tag_errors {
                                            // Send error to Discord channel
                                            if let Err(e) =
                                                channel_id.say(&wow_to_discord_ctx.http, error_msg).await
                                            {
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
                                                if let Err(e) =
                                                    wow_to_discord_ctx.wow_tx.send(whisper_msg)
                                                {
                                                    warn!(
                                                        "Failed to send tag error whisper to WoW: {}",
                                                        e
                                                    );
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

        // Spawn command response forwarding task
        let cmd_bridge = self.bridge.clone();
        let cmd_cache = ctx.cache.clone();
        let mut cmd_rx = task_channels.cmd_response_rx;
        let mut cmd_shutdown = self.shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    response_opt = cmd_rx.recv() => {
                        match response_opt {
                            Some(response) => {
                                if *cmd_shutdown.borrow() {
                                    break;
                                }

                                // Apply bridge formatting (e.g., MOTD format)
                                let formatted = cmd_bridge.format_command_response(&response.content);

                                // Apply post-bridge processing (emojis, markdown escape)
                                let result = cmd_response_ctx.resolver.process_post_bridge(
                                    &cmd_cache,
                                    serenity::model::id::ChannelId::new(response.channel_id),
                                    &formatted,
                                    cmd_response_ctx.self_user_id,
                                );

                                // Send to Discord
                                let channel = serenity::model::id::ChannelId::new(response.channel_id);
                                if let Err(e) = channel.say(&cmd_response_ctx.http, &result.message).await {
                                    error!("Failed to send command response to Discord: {}", e);
                                }
                            }
                            None => break,
                        }
                    }
                    _ = cmd_shutdown.changed() => {
                        if *cmd_shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
            info!("Command response forwarding task ended");
        });

        info!("All background tasks spawned successfully");
    }
}

#[async_trait]
impl EventHandler for BridgeHandler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore our own messages and bots
        if msg.author.id == ctx.cache.current_user().id || msg.author.bot {
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

        // Get resolved state (if available)
        let resolved = {
            let guard = self.resolved_state.lock().unwrap();
            guard.clone()
        };

        let Some(state) = resolved else {
            // State not resolved yet, ignore message
            debug!("Ignoring message - state not yet resolved");
            return;
        };

        // Create context for Discord -> WoW handling
        let d2w_ctx = DiscordToWowContext::from_resolved(&state);

        // Check for !commands first
        if content.len() <= 100 && (content.starts_with('!') || content.starts_with('?')) {
            let channel_name = msg.channel_id.name(&ctx).await.unwrap_or_default();
            if d2w_ctx.command_allowed_in_channel(&channel_name, msg.channel_id.get()) {
                match self.command_handler.handle_command(&ctx, &msg, content).await {
                    Ok(true) => return, // Command was handled
                    Ok(false) => {}     // Not a known command, continue
                    Err(e) => {
                        error!("Command handler error: {}", e);
                        return;
                    }
                }
            }
        }

        // Check for .dot commands
        if content.len() <= 100 && content.starts_with('.') {
            let channel_name = msg.channel_id.name(&ctx).await.unwrap_or_default();
            let should_send_directly = d2w_ctx.should_send_dot_command_directly(content)
                && d2w_ctx.command_allowed_in_channel(&channel_name, msg.channel_id.get());

            if should_send_directly {
                let discord_msg = DiscordMessage {
                    sender: "".to_string(),
                    content: content.to_string(),
                    channel_id: msg.channel_id.get(),
                    channel_name: "".to_string(),
                };
                if let Some(outgoing) = self.bridge.handle_discord_to_wow_directly(&discord_msg) {
                    if let Err(e) = d2w_ctx.wow_tx.send(outgoing) {
                        error!("Failed to send dot command to WoW: {}", e);
                    }
                }
                return;
            }
        }

        // Process as a regular message
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
        let processed = d2w_ctx
            .resolver
            .process_discord_to_wow(&full_content, &ctx.cache);

        // Create DiscordMessage and use Bridge to process, filter, and format
        let discord_msg = DiscordMessage {
            sender: sender.clone(),
            content: processed,
            channel_id: msg.channel_id.get(),
            channel_name: d2w_ctx
                .discord_to_wow
                .get(&msg.channel_id)
                .map(|c| c.discord_channel_name.clone())
                .unwrap_or_default(),
        };

        let outgoing = self.bridge.handle_discord_to_wow(&discord_msg);
        for wow_msg in outgoing {
            if let Err(e) = d2w_ctx.wow_tx.send(wow_msg) {
                error!("Failed to send message to WoW: {}", e);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        // Send ready data to guild_create
        let ready_data = ReadyData {
            self_user_id: ready.user.id.get(),
        };

        let tx = {
            let mut guard = self.ready_tx.lock().unwrap();
            guard.take()
        };

        if let Some(tx) = tx {
            if tx.send(ready_data).is_err() {
                error!("Failed to send ready data - receiver dropped");
            }
        }

        info!("Waiting for guild data to resolve channels...");

        // NOTE: We don't spawn background tasks here.
        // They are spawned in guild_create() after state resolution.
        let _ = ctx; // Silence unused warning - ctx is used in guild_create
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        info!(
            "Received guild data for '{}' ({} channels)",
            guild.name,
            guild.channels.len()
        );

        // Wait for ready data
        let ready_rx = {
            let mut guard = self.ready_rx.lock().unwrap();
            guard.take()
        };

        let ready_data = match ready_rx {
            Some(rx) => match rx.await {
                Ok(data) => data,
                Err(_) => {
                    error!("Failed to receive ready data - sender dropped");
                    return;
                }
            },
            None => {
                // Already processed in a previous guild_create call
                debug!("State already resolved, skipping duplicate guild_create");
                return;
            }
        };

        // Take pending state
        let pending_state = {
            let mut guard = self.pending_state.lock().unwrap();
            guard.take()
        };

        let Some(pending) = pending_state else {
            debug!("Pending state already consumed");
            return;
        };

        // Take task channels
        let task_channels = {
            let mut guard = self.task_channels.lock().unwrap();
            guard.take()
        };

        let Some(channels) = task_channels else {
            error!("Task channels already consumed");
            return;
        };

        // Log pending channel configs
        let pending_names: Vec<&str> = pending
            .pending_channel_configs
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect();
        info!("Resolving {} pending channels: {:?}", pending_names.len(), pending_names);

        // Convert HashMap values to Vec for resolution
        let guild_channels: Vec<_> = guild.channels.values().cloned().collect();

        // Resolve state
        let resolved = pending.resolve(&guild_channels, ctx.http.clone(), ready_data.self_user_id);

        // Store resolved state for message handler
        {
            let mut guard = self.resolved_state.lock().unwrap();
            *guard = Some(Arc::new(resolved.clone()));
        }

        // Spawn background tasks with owned contexts
        self.spawn_background_tasks(&ctx, &resolved, channels);

        info!("Bridge initialization complete - all systems operational");
    }
}
