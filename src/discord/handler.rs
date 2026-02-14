//! Discord message event handling.
//!
//! Provides the event handler for Discord messages and manages
//! the message flow between Discord and WoW.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;

use crate::bridge::{
    Bridge, PendingBridgeState, ResolvedBridgeState,
};
use crate::common::messages::DashboardEvent;
use crate::common::{ActivityStatus, BridgeMessage, DiscordMessage};
use crate::config::types::GuildDashboardConfig;
use crate::discord::commands::{CommandHandler, CommandResponse, WowCommand};
use crate::discord::dashboard::DashboardRenderer;

/// Channels bundle for background tasks.
/// These are consumed when tasks are spawned (moved into the tasks).
pub struct TaskChannels {
    pub wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    pub cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
    pub status_rx: mpsc::UnboundedReceiver<ActivityStatus>,
    pub dashboard_rx: mpsc::UnboundedReceiver<DashboardEvent>,
}

/// Discord event handler.
pub struct BridgeHandler {
    /// Bridge for message routing/formatting (immutable, shared).
    bridge: Arc<Bridge>,
    /// Pending state waiting for resolution (consumed in guild_create).
    pending_state: Option<PendingBridgeState>,
    /// Command handler for Discord commands.
    command_handler: CommandHandler,
    /// Dashboard renderer for updating dashboard messages.
    dashboard_renderer: DashboardRenderer,
    /// Resolved state (available after guild_create, for message() handler and channel processing).
    resolved_state: Option<Arc<ResolvedBridgeState>>,
    /// Signal sent to main after guild_create() completes initialization.
    init_complete_tx: Option<oneshot::Sender<()>>,
}

impl BridgeHandler {
    /// Create a new bridge handler.
    ///
    /// The handler takes ownership of pending state.
    pub fn new(
        bridge: Arc<Bridge>,
        pending_state: PendingBridgeState,
        command_tx: mpsc::UnboundedSender<WowCommand>,
        dashboard_config: GuildDashboardConfig,
        init_complete_tx: oneshot::Sender<()>,
    ) -> Self {
        let dashboard_renderer = DashboardRenderer::new(dashboard_config);
        Self {
            bridge,
            pending_state: Some(pending_state),
            command_handler: CommandHandler::new(command_tx),
            dashboard_renderer,
            resolved_state: None,
            init_complete_tx: Some(init_complete_tx),
        }
    }

    /// Process a WoW message and forward it to Discord channels.
    pub async fn handle_wow_message(&mut self, context: &Context, msg: BridgeMessage) {
        let resolved = match &self.resolved_state {
            Some(resolved) => resolved,
            None => {
                debug!("Cannot process WoW message - state not resolved");
                return;
            }
        };

        let cache = context.cache.clone();

        // Pre-process WoW content (resolve links, strip colors/textures)
        let processed_content = resolved.resolver.process_pre_bridge(&msg.content);

        // Process and filter message through Bridge
        let results = self.bridge.handle_wow_to_discord(
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
            if let Some(channel_configs) = resolved.wow_to_discord.get(&key) {
                for config in channel_configs {
                    if config.discord_channel_name == discord_channel_name {
                        if let Some(channel_id) = config.discord_channel_id {
                            // Apply post-bridge processing (emojis, tags, markdown escape)
                            let (final_message, tag_errors) = if msg.sender.is_some() {
                                let result = resolved.resolver.process_post_bridge(
                                    &cache,
                                    channel_id,
                                    &formatted,
                                    resolved.self_user_id,
                                );
                                (result.message, result.errors)
                            } else {
                                (formatted.clone(), Vec::new())
                            };

                            // Send the message to Discord
                            match channel_id.say(context.http.clone(), &final_message).await {
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
                            if resolved.enable_tag_failed_notifications && !tag_errors.is_empty() {
                                for error_msg in &tag_errors {
                                    // Send error to Discord channel
                                    if let Err(e) = channel_id.say(context.http.clone(), error_msg).await {
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
                                        if let Err(e) = resolved.wow_tx.send(whisper_msg) {
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

    /// Update Discord bot activity.
    pub async fn handle_status_update(&mut self, context: &Context, status: ActivityStatus) {
        use serenity::gateway::ActivityData;

        match status {
            ActivityStatus::Connecting => {
                let activity = ActivityData::custom("Connecting...");
                context.set_activity(Some(activity));
            }
            ActivityStatus::Disconnected => {
                let activity = ActivityData::custom("Offline");
                context.set_activity(Some(activity));
            }
            ActivityStatus::ConnectedToRealm(realm) => {
                let activity = ActivityData::custom(realm);
                context.set_activity(Some(activity));
            }
            ActivityStatus::GuildStats { online_count } => {
                let plural = if online_count == 1 { "" } else { "s" };
                let text = if online_count == 0 {
                    "Currently no guildies online".to_string()
                } else {
                    format!("{} guildie{} online", online_count, plural)
                };
                let activity = ActivityData::watching(text);
                context.set_activity(Some(activity));
            }
        }
    }

    /// Process a command response and send it to Discord.
    pub async fn handle_command_response(&mut self, context: &Context, response: CommandResponse) {
        let resolved = match &self.resolved_state {
            Some(resolved) => resolved,
            None => {
                debug!("Cannot process command response - state not resolved");
                return;
            }
        };

        let cache = context.cache.clone();

        // Apply bridge formatting (e.g., MOTD format)
        let formatted = self.bridge.format_command_response(&response.content);

        // Apply post-bridge processing (emojis, markdown escape)
        let result = resolved.resolver.process_post_bridge(
            &cache,
            serenity::model::id::ChannelId::new(response.channel_id),
            &formatted,
            resolved.self_user_id,
        );

        // Send to Discord (with chunking for large messages)
        let channel = serenity::model::id::ChannelId::new(response.channel_id);
        const MAX_MESSAGE_LENGTH: usize = 1900; // Leave some buffer under Discord's 2000 limit

        let message = &result.message;
        if message.len() <= MAX_MESSAGE_LENGTH {
            // Single message - send directly
            if let Err(e) = channel.say(context.http.clone(), message).await {
                error!("Failed to send command response to Discord: {}", e);
            }
        } else {
            // Large message - split into chunks at newline boundaries
            let chunks = split_message(message, MAX_MESSAGE_LENGTH);
            for (i, chunk) in chunks.iter().enumerate() {
                if let Err(e) = channel.say(context.http.clone(), chunk).await {
                    error!("Failed to send command response chunk {} to Discord: {}", i + 1, e);
                    break;
                }
                // Small delay between chunks to avoid rate limits
                if i < chunks.len() - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Process a dashboard event and update the dashboard message.
    pub async fn handle_dashboard_event(&mut self, context: &Context, event: DashboardEvent) {
        let resolved = match &self.resolved_state {
            Some(resolved) => resolved.clone(),
            None => {
                debug!("Cannot process dashboard event - state not resolved");
                return;
            }
        };

        match event {
            DashboardEvent::Update(data) => {
                self.dashboard_renderer.update(context, resolved.dashboard_channel_id, data).await;
            }
            DashboardEvent::SetOffline => {
                self.dashboard_renderer.set_offline(context, resolved.dashboard_channel_id).await;
            }
        }
    }

    pub async fn handle_message(&mut self, context: Context, msg: Message) {
        // Ignore our own messages and bots
        if msg.author.id == context.cache.current_user().id {
            return;
        }

        /*if msg.author.bot {
            return;
        }*/

        // Only handle guild (server) messages
        if msg.guild_id.is_none() {
            return;
        }

        let content = msg.content.trim();
        if content.is_empty() && msg.attachments.is_empty() {
            return;
        }

        // Get resolved state (if available)
        let resolved = match &self.resolved_state {
            Some(state) => state.clone(),
            None => {
                // State not resolved yet, ignore message
                debug!("Ignoring message - state not yet resolved");
                return;
            }
        };

        // Check for !commands first
        if content.len() <= 100 && (content.starts_with('!') || content.starts_with('?')) {
            let channel_name = msg.channel_id.name(&context).await.unwrap_or_default();
            if resolved.command_allowed_in_channel(&channel_name, msg.channel_id.get()) {
                match self.command_handler.handle_command(&context, &msg, content).await {
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
            let channel_name = msg.channel_id.name(&context).await.unwrap_or_default();
            let should_send_directly = resolved.should_send_dot_command_directly(content)
                && resolved.command_allowed_in_channel(&channel_name, msg.channel_id.get());

            if should_send_directly {
                let discord_msg = DiscordMessage {
                    sender: "".to_string(),
                    content: content.to_string(),
                    channel_id: msg.channel_id.get(),
                    channel_name: "".to_string(),
                };
                if let Some(outgoing) = self.bridge.handle_discord_to_wow_directly(&discord_msg) {
                    if let Err(e) = resolved.wow_tx.send(outgoing) {
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
        let processed = resolved
            .resolver
            .process_discord_to_wow(&full_content, &context.cache);

        // Create DiscordMessage and use Bridge to process, filter, and format
        let discord_msg = DiscordMessage {
            sender: sender.clone(),
            content: processed,
            channel_id: msg.channel_id.get(),
            channel_name: resolved
                .discord_to_wow
                .get(&msg.channel_id)
                .map(|c| c.discord_channel_name.clone())
                .unwrap_or_default(),
        };

        let outgoing = self.bridge.handle_discord_to_wow(&discord_msg);
        for wow_msg in outgoing {
            if let Err(e) = resolved.wow_tx.send(wow_msg) {
                error!("Failed to send message to WoW: {}", e);
            }
        }
    }

    pub async fn handle_guild_create(&mut self, context: Context, ready: Ready, guild: Guild) {
        info!(
            "Received guild data for '{}' ({} channels)",
            guild.name,
            guild.channels.len()
        );

        // Take pending state
        let pending = match self.pending_state.take() {
            Some(pending) => pending,
            None => {
                debug!("Pending state already consumed");
                return;
            }
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
        let resolved = pending.resolve(&guild_channels, ready.user.id.into());

        // Store resolved state for message handler
        self.resolved_state = Some(Arc::new(resolved));

        // Signal main that initialization succeeded
        if let Some(tx) = self.init_complete_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Split a large message into chunks at newline boundaries.
/// Ensures each chunk is under the max_length limit.
fn split_message(message: &str, max_length: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for line in message.lines() {
        // If a single line is too long, we have to split it
        if line.len() > max_length {
            // Flush current chunk first
            if !current_chunk.is_empty() {
                chunks.push(current_chunk.clone());
                current_chunk.clear();
            }

            // Split the long line into chunks
            let line_bytes = line.as_bytes();
            for chunk in line_bytes.chunks(max_length) {
                chunks.push(String::from_utf8_lossy(chunk).to_string());
            }
        } else if current_chunk.len() + line.len() + 1 > max_length {
            // Adding this line would exceed limit, flush current chunk
            chunks.push(current_chunk.clone());
            current_chunk = line.to_string();
        } else {
            // Add line to current chunk
            if !current_chunk.is_empty() {
                current_chunk.push('\n');
            }
            current_chunk.push_str(line);
        }
    }

    // Don't forget the last chunk
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}
