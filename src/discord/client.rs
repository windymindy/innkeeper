//! Discord bot client abstraction.
//!
//! Provides a high-level interface for creating and running the Discord bot,
//! hiding serenity implementation details from the rest of the application.

use std::sync::Arc;
use std::time::Duration;

use serenity::prelude::*;
use serenity::async_trait;
use serenity::Client;
use serenity::http::HttpBuilder;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use backon::BackoffBuilder;

use crate::bridge::{Bridge, ChannelConfig, PendingBridgeState};
use crate::bridge::orchestrator::parse_channel_config;
use crate::common::{ActivityStatus, BridgeMessage};
use crate::common::messages::DashboardEvent;
use crate::config::types::{Config, Direction, GuildDashboardConfig};
use crate::discord::commands::{CommandResponse, WowCommand};
use crate::discord::handler::{BridgeHandler, TaskChannels};

#[derive(Debug, Clone)]
pub enum DiscordBotEvent {
    /// Bot connected and ready.
    Ready(Ready),
    /// Guild data received.
    GuildCreate {
        context: Context,
        guild: Guild,
    },
    /// Message received.
    Message {
        context: Context,
        message: serenity::model::channel::Message,
    },
    Disconnected
}

struct DiscordBotEvents {
    discord_events_tx: mpsc::UnboundedSender<DiscordBotEvent>,
}

impl DiscordBotEvents {
    fn new(discord_events_tx: mpsc::UnboundedSender<DiscordBotEvent>) -> Self {
        Self { discord_events_tx }
    }
}

#[async_trait]
impl EventHandler for DiscordBotEvents {
    async fn ready(&self, _context: Context, ready: Ready) {
        if let Err(error) = self.discord_events_tx.send(DiscordBotEvent::Ready(ready)) {
            warn!("Failed to process discord event: {}", error);
        }
    }

    async fn guild_create(&self, context: Context, guild: Guild, _is_new: Option<bool>) {
        if let Err(error) = self.discord_events_tx.send(DiscordBotEvent::GuildCreate { context, guild }) {
            warn!("Failed to process discord event: {}", error);
        }
    }

    async fn message(&self, context: Context, message: serenity::model::channel::Message) {
        if let Err(error) = self.discord_events_tx.send(DiscordBotEvent::Message { context, message }) {
            warn!("Failed to process discord event: {}", error);
        }
    }
}

/// Channels for Discord bot communication.
pub struct DiscordChannels {
    /// Sender for outgoing WoW messages (Discord -> WoW).
    pub outgoing_wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Receiver for incoming WoW messages (WoW -> Discord).
    pub wow_to_discord_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    /// Sender for commands from Discord.
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
    /// Receiver for command responses from game client.
    pub cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
    /// Receiver for status updates from game client.
    pub status_rx: mpsc::UnboundedReceiver<ActivityStatus>,
    /// Receiver for dashboard updates from game client.
    pub dashboard_rx: mpsc::UnboundedReceiver<DashboardEvent>,
    /// Receiver for shutdown signal.
    pub shutdown_rx: watch::Receiver<bool>,
}

/// Builder for creating the Discord bot.
pub struct DiscordBotBuilder {
    token: String,
    config: Config,
    channels: DiscordChannels,
    bridge: Arc<Bridge>,
}

impl DiscordBotBuilder {
    /// Create a new Discord bot builder.
    pub fn new(
        token: String,
        config: Config,
        channels: DiscordChannels,
        bridge: Arc<Bridge>,
    ) -> Self {
        Self {
            token,
            config,
            channels,
            bridge,
        }
    }

    /// Build the Discord bot.
    pub async fn build(self, init_complete_tx: oneshot::Sender<()>) -> anyhow::Result<DiscordBot> {
        // Build pending channel configs from config
        let mut pending_configs: Vec<(String, Direction, ChannelConfig)> = Vec::new();

        for channel in &self.config.chat.channels {
            let (chat_type, wow_channel_name) = parse_channel_config(&channel.wow);
            let wow_chat_type = chat_type.to_id();

            let channel_config = ChannelConfig {
                discord_channel_id: None,
                discord_channel_name: channel.discord.channel.clone(),
                wow_chat_type,
                wow_channel_name,
            };

            pending_configs.push((
                channel.discord.channel.clone(),
                channel.direction,
                channel_config,
            ));
        }

        info!(
            "Configured {} channel mappings",
            self.config.chat.channels.len()
        );

        // Create pending bridge state
        let pending_state = PendingBridgeState::new(
            pending_configs,
            self.channels.outgoing_wow_tx.clone(),
            self.channels.command_tx.clone(),
            self.config.discord.enable_dot_commands,
            self.config.discord.dot_commands_whitelist.clone(),
            self.config.discord.enable_commands_channels.clone(),
            self.config.discord.enable_markdown,
            self.config.discord.enable_tag_failed_notifications,
            Some(self.config.guild_dashboard.clone()),
        );

        // Create task channels bundle
        let task_channels = TaskChannels {
            wow_rx: self.channels.wow_to_discord_rx,
            cmd_response_rx: self.channels.cmd_response_rx,
            status_rx: self.channels.status_rx,
            dashboard_rx: self.channels.dashboard_rx,
        };

        let (discord_events_tx, discord_events_rx) = mpsc::unbounded_channel::<DiscordBotEvent>();

        let client = build_client(&self.token, discord_events_tx.clone()).await?;

        let handler = BridgeHandler::new(
            self.bridge.clone(),
            pending_state,
            self.channels.command_tx.clone(),
            self.config.guild_dashboard.clone(),
            init_complete_tx,
        );

        Ok(DiscordBot {
            client: Some(client),
            token: self.token,
            dashboard_config: self.config.guild_dashboard,
            outgoing_wow_tx: self.channels.outgoing_wow_tx,
            command_tx: self.channels.command_tx,
            bridge: self.bridge,
            handler,
            discord_events_rx,
            discord_events_tx,
            task_channels,
            shutdown_rx: self.channels.shutdown_rx,
        })
    }
}

async fn build_client(token: &String, discord_events_tx: mpsc::UnboundedSender<DiscordBotEvent>) -> anyhow::Result<Client> {
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_PRESENCES;

    // Build a custom reqwest client with timeout settings
    let reqwest_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .build()?;

    // Build the Serenity HTTP client with our custom reqwest client
    let http = HttpBuilder::new(token)
        .client(reqwest_client)
        .build();

    let events = DiscordBotEvents::new(discord_events_tx);
    let client = serenity::client::ClientBuilder::new_with_http(http, intents)
        .event_handler(events)
        .await?;
    Ok(client)
}

pub struct DiscordBot {
    client: Option<Client>,
    token: String,
    dashboard_config: GuildDashboardConfig,
    outgoing_wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    command_tx: mpsc::UnboundedSender<WowCommand>,
    bridge: Arc<Bridge>,
    handler: BridgeHandler,
    discord_events_rx: mpsc::UnboundedReceiver<DiscordBotEvent>,
    discord_events_tx: mpsc::UnboundedSender<DiscordBotEvent>,
    task_channels: TaskChannels,
    shutdown_rx: watch::Receiver<bool>,
}

impl DiscordBot {
    pub async fn run(mut self) {
        // Extract shard manager before we move client into run_connection
        let shard_manager = self.client.as_ref()
            .map(|c| c.shard_manager.clone());
        let client = &mut self.client;
        let discord_events_rx = &mut self.discord_events_rx;
        let handler = &mut self.handler;
        let task_channels = &mut self.task_channels;
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::select! {
            _ = Self::run_connection(client, &self.token, &self.discord_events_tx) => {},
            _ = Self::process_events(discord_events_rx, handler, task_channels, &mut self.shutdown_rx) => {},
            _ = async {
                // Wait for shutdown signal
                loop {
                    shutdown_rx.changed().await.ok();
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                // Gracefully shutdown Discord gateway
                if let Some(ref manager) = shard_manager {
                    info!("Initiating graceful Discord shutdown...");
                    manager.shutdown_all().await;
                    info!("Discord shutdown complete");
                }
            } => {}
        }
        info!("Discord task ended");
    }

    async fn run_connection(client: &mut Option<Client>, token: &String, discord_events_tx: &mpsc::UnboundedSender<DiscordBotEvent>) {
        /// Create an exponential backoff iterator for Discord reconnection.
        /// 5s initial, 5min max, factor 1.1, with jitter, unlimited retries.
        fn discord_backoff() -> impl Iterator<Item = Duration> {
            backon::ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(5))
                .with_max_delay(Duration::from_mins(5))
                .with_factor(1.1)
                .with_jitter()
                .without_max_times()
                .build()
        }

        let mut backoff = discord_backoff();

        loop {
            info!("Connecting to Discord...");

            let mut client = match client.take() {
                Some(client) => client,
                None => {
                    // serenity mostly handles reconnections itself.
                    match build_client(token, discord_events_tx.clone()).await {
                        Ok(client) => {
                            backoff = discord_backoff();
                            client
                        }
                        Err(e) => {
                            error!("Failed to rebuild Discord client: {}", e);
                            let delay = backoff.next().unwrap_or(Duration::from_mins(5));
                            warn!("Retrying in {:.1}s...", delay.as_secs_f64());
                            sleep(delay).await;
                            continue;
                        }
                    }
                }
            };

            // Run the client
            match client.start().await {
                Ok(()) => {
                    info!("Discord client disconnected normally");
                    if let Err(error) = discord_events_tx.send(DiscordBotEvent::Disconnected) {
                        warn!("Failed to process discord event: {}", error);
                    }
                    break;
                }
                Err(e) => {
                    error!("Discord client error: {}", e);
                    let delay = backoff.next().unwrap_or(Duration::from_mins(5));
                    warn!(
                        "Discord disconnected. Reconnecting in {:.1}s...",
                        delay.as_secs_f64(),
                    );
                    if let Err(error) = discord_events_tx.send(DiscordBotEvent::Disconnected) {
                        warn!("Failed to process discord event: {}", error);
                    }
                    sleep(delay).await;
                }
            }
        }
    }

    async fn process_events(
        discord_events_rx: &mut mpsc::UnboundedReceiver<DiscordBotEvent>,
        handler: &mut BridgeHandler,
        task_channels: &mut TaskChannels,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) {
        let mut discord_user = None;
        let mut discord_connection = None;

        loop {
            tokio::select! {
                // Discord events
                event = discord_events_rx.recv() => {
                    match event {
                        Some(event) => {
                            match event {
                                DiscordBotEvent::Ready(ready) => {
                                    info!("Discord bot connected as {}", ready.user.name);
                                    discord_user = Some(ready);
                                }
                                DiscordBotEvent::GuildCreate { context, guild } => {
                                    let ready = match discord_user.as_ref() {
                                        Some(v) => v,
                                        None => {
                                            error!("Received GuildCreate event before Ready event");
                                            return;
                                        }
                                    };
                                    handler.handle_guild_create(context.clone(), ready.clone(), guild).await;
                                    discord_connection = Some(context);
                                }
                                DiscordBotEvent::Message { context, message } => {
                                    handler.handle_message(context, message).await;
                                }
                                DiscordBotEvent::Disconnected => {
                                    discord_user = None;
                                    discord_connection = None;
                                }
                            }
                        }
                        None => {
                            debug!("Discord events channel closed.");
                            break;
                        }
                    }
                }

                // WoW -> Discord messages (drop if not connected)
                message = task_channels.wow_rx.recv() => {
                    match message {
                        Some(message) => {
                            if let Some(ref context) = discord_connection {
                                handler.handle_wow_message(context, message).await;
                            } else {
                                debug!("Dropping WoW message - Discord not connected");
                            }
                        }
                        None => {
                            warn!("WoW message channel closed");
                            break;
                        }
                    }
                }

                // Status updates (drop if not connected)
                message = task_channels.status_rx.recv() => {
                    match message {
                        Some(message) => {
                            if let Some(ref context) = discord_connection {
                                handler.handle_status_update(context, message).await;
                            } else {
                                debug!("Dropping status update - Discord not connected");
                            }
                        }
                        None => {
                            warn!("Status update channel closed");
                            break;
                        }
                    }
                }

                // Command responses (drop if not connected)
                message = task_channels.cmd_response_rx.recv() => {
                    match message {
                        Some(message) => {
                            if let Some(ref context) = discord_connection {
                                handler.handle_command_response(context, message).await;
                            } else {
                                debug!("Dropping command response - Discord not connected");
                            }
                        }
                        None => {
                            warn!("Command response channel closed");
                            break;
                        }
                    }
                }

                // Dashboard events (drop if not connected)
                event = task_channels.dashboard_rx.recv() => {
                    match event {
                        Some(event) => {
                            if let Some(ref context) = discord_connection {
                                handler.handle_dashboard_event(context, event).await;
                            } else {
                                debug!("Dropping dashboard event - Discord not connected");
                            }
                        }
                        None => {
                            warn!("Dashboard event channel closed");
                            break;
                        }
                    }
                }

                // Shutdown signal
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Shutdown signal received, stopping event processing");
                        break;
                    }
                }
            }
        }
    }
}
