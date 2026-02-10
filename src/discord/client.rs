//! Discord bot client abstraction.
//!
//! Provides a high-level interface for creating and running the Discord bot,
//! hiding serenity implementation details from the rest of the application.

use std::sync::Arc;
use std::time::Duration;

use backon::BackoffBuilder;
use serenity::all::Http;
use serenity::model::gateway::GatewayIntents;
use serenity::Client;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::bridge::{Bridge, ChannelConfig, PendingBridgeState};
use crate::bridge::orchestrator::parse_channel_config;
use crate::common::{ActivityStatus, BridgeMessage};
use crate::common::messages::DashboardEvent;
use crate::config::types::{Config, GuildDashboardConfig};
use crate::discord::commands::{CommandResponse, WowCommand};
use crate::discord::handler::{BridgeHandler, TaskChannels};

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
        let mut pending_configs: Vec<(String, String, ChannelConfig)> = Vec::new();

        for channel in &self.config.chat.channels {
            let (chat_type, wow_channel_name) = parse_channel_config(&channel.wow);
            let wow_chat_type = chat_type.to_id();

            let channel_config = ChannelConfig {
                discord_channel_id: None,
                discord_channel_name: channel.discord.channel.clone(),
                wow_chat_type,
                wow_channel_name,
                format_wow_to_discord: channel
                    .discord
                    .format
                    .clone()
                    .unwrap_or_else(|| "[%user]: %message".to_string()),
                format_discord_to_wow: channel
                    .wow
                    .format
                    .clone()
                    .unwrap_or_else(|| "%user: %message".to_string()),
            };

            pending_configs.push((
                channel.discord.channel.clone(),
                channel.direction.clone(),
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

        // Build intents
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES;

        // Create handler
        let handler = BridgeHandler::new(
            self.bridge.clone(),
            pending_state,
            task_channels,
            self.channels.command_tx.clone(),
            self.config.guild_dashboard.clone(),
            self.channels.shutdown_rx,
            init_complete_tx,
        );

        // Build client
        let client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await?;

        let http = client.http.clone();

        Ok(DiscordBot {
            client: Some(client),
            http,
            token: self.token,
            intents,
            dashboard_config: self.config.guild_dashboard,
            outgoing_wow_tx: self.channels.outgoing_wow_tx,
            command_tx: self.channels.command_tx,
            bridge: self.bridge,
        })
    }
}

/// Discord bot instance with reconnection support.
pub struct DiscordBot {
    client: Option<Client>,
    http: Arc<Http>,
    token: String,
    intents: GatewayIntents,
    dashboard_config: GuildDashboardConfig,
    outgoing_wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    command_tx: mpsc::UnboundedSender<WowCommand>,
    bridge: Arc<Bridge>,
}

impl DiscordBot {
    /// Get the HTTP client for sending messages.
    pub fn http(&self) -> Arc<Http> {
        self.http.clone()
    }

    /// Run the Discord bot with automatic reconnection.
    pub async fn run(mut self) {
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

            let mut client = match self.client.take() {
                Some(c) => c,
                None => {
                    // Need to rebuild the client for reconnection
                    // Note: On reconnection, we lose the pending state and task channels.
                    // This is a limitation - reconnection creates dummy channels.
                    warn!("Rebuilding Discord client for reconnection (some functionality may be limited)...");

                    // Create dummy channels for reconnection
                    let (dummy_wow_tx, dummy_wow_rx) = mpsc::unbounded_channel::<BridgeMessage>();
                    let (dummy_cmd_response_tx, dummy_cmd_response_rx) =
                        mpsc::unbounded_channel::<CommandResponse>();
                    let (dummy_status_tx, dummy_status_rx) =
                        mpsc::unbounded_channel::<ActivityStatus>();
                    let (dummy_dashboard_tx, dummy_dashboard_rx) =
                        mpsc::unbounded_channel::<DashboardEvent>();
                    let (dummy_shutdown_tx, dummy_shutdown_rx) = watch::channel(false);

                    // Create minimal pending state
                    let pending_state = PendingBridgeState::new(
                        Vec::new(), // No pending configs on reconnection
                        self.outgoing_wow_tx.clone(),
                        self.command_tx.clone(),
                        false,
                        None,
                        None,
                        false,
                        false,
                        Some(self.dashboard_config.clone()),
                    );

                    let task_channels = TaskChannels {
                        wow_rx: dummy_wow_rx,
                        cmd_response_rx: dummy_cmd_response_rx,
                        status_rx: dummy_status_rx,
                        dashboard_rx: dummy_dashboard_rx,
                    };

                    // Create a dummy init signal for reconnection (already fired on first connect)
                    let (dummy_init_tx, _) = oneshot::channel();

                    let handler = BridgeHandler::new(
                        self.bridge.clone(),
                        pending_state,
                        task_channels,
                        self.command_tx.clone(),
                        self.dashboard_config.clone(),
                        dummy_shutdown_rx,
                        dummy_init_tx,
                    );

                    match Client::builder(&self.token, self.intents)
                        .event_handler(handler)
                        .await
                    {
                        Ok(client) => {
                            self.http = client.http.clone();
                            // Clean up dummy senders
                            drop(dummy_wow_tx);
                            drop(dummy_cmd_response_tx);
                            drop(dummy_status_tx);
                            drop(dummy_dashboard_tx);
                            drop(dummy_shutdown_tx);
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
                    break;
                }
                Err(e) => {
                    error!("Discord client error: {}", e);
                    let delay = backoff.next().unwrap_or(Duration::from_mins(5));
                    warn!(
                        "Discord disconnected. Reconnecting in {:.1}s...",
                        delay.as_secs_f64(),
                    );
                    sleep(delay).await;
                }
            }
        }
    }
}
