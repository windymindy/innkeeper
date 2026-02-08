//! Discord bot client abstraction.
//!
//! Provides a high-level interface for creating and running the Discord bot,
//! hiding serenity implementation details from the rest of the application.

use std::sync::Arc;
use std::time::Duration;

use serenity::all::Http;
use serenity::Client;
use serenity::model::gateway::GatewayIntents;
use tokio::sync::{mpsc, watch, RwLock};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::bridge::{Bridge, BridgeState, ChannelConfig};
use crate::common::{ActivityStatus, BridgeMessage};
use crate::config::types::{Config, GuildDashboardConfig};
use crate::bridge::orchestrator::parse_channel_config;
use crate::discord::commands::CommandResponse;

use super::commands::WowCommand;
use super::handler::BridgeHandler;
use super::resolver::MessageResolver;

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
    pub dashboard_rx: mpsc::UnboundedReceiver<crate::common::messages::DashboardEvent>,
    /// Receiver for shutdown signal.
    pub shutdown_rx: watch::Receiver<bool>,
}

/// Shared state that persists across reconnections.
struct SharedBridgeState {
    state: Arc<RwLock<BridgeState>>,
}

/// Configuration for building Discord clients.
struct ClientConfig {
    token: String,
    intents: GatewayIntents,
    shared_state: Arc<RwLock<BridgeState>>,
    dashboard_config: GuildDashboardConfig,
    shutdown_rx: watch::Receiver<bool>,
}

impl ClientConfig {
    /// Build a new serenity Client with the stored configuration.
    async fn build_client(
        &self,
        wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
        command_tx: mpsc::UnboundedSender<WowCommand>,
        cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
        status_rx: mpsc::UnboundedReceiver<ActivityStatus>,
        dashboard_rx: mpsc::UnboundedReceiver<crate::common::messages::DashboardEvent>,
    ) -> anyhow::Result<Client> {
        let handler = BridgeHandler::new(
            wow_rx,
            command_tx,
            cmd_response_rx,
            status_rx,
            dashboard_rx,
            self.dashboard_config.clone(),
            self.shutdown_rx.clone(),
        );

        let client = Client::builder(&self.token, self.intents)
            .event_handler(handler)
            .await?;

        // Store the shared state in the new client
        {
            let mut data = client.data.write().await;
            data.insert::<BridgeState>(self.shared_state.clone());
        }

        Ok(client)
    }
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
    pub fn new(token: String, config: Config, channels: DiscordChannels, bridge: Arc<Bridge>) -> Self {
        Self {
            token,
            config,
            channels,
            bridge,
        }
    }
}

impl DiscordBotBuilder {
    /// Build the Discord bot.
    pub async fn build(self) -> anyhow::Result<DiscordBot> {
        // Build channel mappings from config
        let mut pending_configs: Vec<(String, String, ChannelConfig)> = Vec::new();

        for channel in &self.config.chat.channels {
            let (chat_type, wow_channel_name) = parse_channel_config(&channel.wow);
            let wow_chat_type = chat_type.to_id();

            let channel_config = ChannelConfig {
                discord_channel_id: None,
                discord_channel_name: channel.discord.channel.clone(),
                wow_chat_type,
                wow_channel_name,
                // discord.format is used for messages going TO Discord (WoW → Discord)
                format_wow_to_discord: channel
                    .discord
                    .format
                    .clone()
                    .unwrap_or_else(|| "[%user]: %message".to_string()),
                // wow.format is used for messages going TO WoW (Discord → WoW)
                format_discord_to_wow: channel
                    .wow
                    .format
                    .clone()
                    .unwrap_or_else(|| "%user: %message".to_string()),
            };

            pending_configs.push((channel.discord.channel.clone(), channel.direction.clone(), channel_config));
        }

        info!("Configured {} channel mappings", self.config.chat.channels.len());

        // Create bridge state (shared across reconnections)
        let bridge_state = BridgeState {
            wow_to_discord: std::collections::HashMap::new(),
            discord_to_wow: std::collections::HashMap::new(),
            wow_tx: self.channels.outgoing_wow_tx.clone(),
            command_tx: self.channels.command_tx.clone(),
            resolver: MessageResolver::new(self.config.discord.enable_markdown),
            pending_channel_configs: pending_configs,
            enable_dot_commands: self.config.discord.enable_dot_commands,
            dot_commands_whitelist: self.config.discord.dot_commands_whitelist.clone(),
            enable_commands_channels: self.config.discord.enable_commands_channels.clone(),
            http: None,
            self_user_id: None,
            enable_tag_failed_notifications: self.config.discord.enable_tag_failed_notifications,
            dashboard_config: Some(self.config.guild_dashboard.clone()),
            dashboard_channel_id: None,
        };

        let shared_state = Arc::new(RwLock::new(bridge_state));

        // Build intents
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_PRESENCES;

        // Create client config for rebuilding
        let client_config = ClientConfig {
            token: self.token,
            intents,
            shared_state: shared_state.clone(),
            dashboard_config: self.config.guild_dashboard.clone(),
            shutdown_rx: self.channels.shutdown_rx.clone(),
        };

        // Build initial client
        let handler = BridgeHandler::new(
            self.channels.wow_to_discord_rx,
            self.channels.command_tx.clone(),
            self.channels.cmd_response_rx,
            self.channels.status_rx,
            self.channels.dashboard_rx,
            self.config.guild_dashboard.clone(),
            self.channels.shutdown_rx,
        );

        let client = Client::builder(&client_config.token, client_config.intents)
            .event_handler(handler)
            .await?;

        // Store bridge state and bridge in client
        {
            let mut data = client.data.write().await;
            data.insert::<BridgeState>(shared_state.clone());
            data.insert::<Bridge>(self.bridge.clone());
        }

        let http = client.http.clone();

        Ok(DiscordBot {
            client: Some(client),
            http,
            config: client_config,
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
    config: ClientConfig,
    // Keep senders to create new receivers for reconnection
    outgoing_wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    command_tx: mpsc::UnboundedSender<WowCommand>,
    // Bridge reference for formatting
    bridge: Arc<Bridge>,
}

impl DiscordBot {
    /// Get the HTTP client for sending messages.
    pub fn http(&self) -> Arc<Http> {
        self.http.clone()
    }

    

    /// Run the Discord bot with automatic reconnection.
    pub async fn run(self) {
        self.run_internal(None).await;
    }

    /// Run the Discord bot and signal when ready.
    pub async fn run_with_ready_signal(self, ready_tx: tokio::sync::oneshot::Sender<()>) {
        self.run_internal(Some(ready_tx)).await;
    }

    async fn run_internal(mut self, mut ready_tx: Option<tokio::sync::oneshot::Sender<()>>) {
        let mut retry_count = 0u32;
        let max_retries = 10;
        let base_delay = Duration::from_secs(5);
        let max_delay = Duration::from_secs(300); // 5 minutes max

        loop {
            info!("Connecting to Discord... (attempt {})", retry_count + 1);

            // Take the client out of Option
            let mut client = match self.client.take() {
                Some(c) => c,
                None => {
                    // Need to rebuild the client for reconnection
                    info!("Rebuilding Discord client for reconnection...");

                    // Create new channel for WoW -> Discord messages
                    // Note: The sender side (game client) keeps sending to outgoing_wow_tx
                    // We create a new receiver that will be used by the new handler
                    let (_new_wow_tx, new_wow_rx) = mpsc::unbounded_channel::<BridgeMessage>();

                    // Update the shared state with new channel info if needed
                    // The outgoing_wow_tx is already stored in shared state

                    // Create a dummy command response channel for reconnection
                    // (command responses during reconnection will be lost)
                    let (_dummy_cmd_tx, dummy_cmd_rx) = mpsc::unbounded_channel::<CommandResponse>();

                    // Create a dummy status channel for reconnection (status updates lost during reconnect)
                    let (_dummy_status_tx, dummy_status_rx) = mpsc::unbounded_channel::<ActivityStatus>();

                    // Create a dummy dashboard channel for reconnection
                    let (_dummy_dashboard_tx, dummy_dashboard_rx) = mpsc::unbounded_channel::<crate::common::messages::DashboardEvent>();

                    match self.config.build_client(new_wow_rx, self.command_tx.clone(), dummy_cmd_rx, dummy_status_rx, dummy_dashboard_rx).await {
                        Ok(client) => {
                            // Update HTTP reference
                            self.http = client.http.clone();
                            client
                        }
                        Err(e) => {
                            error!("Failed to rebuild Discord client: {}", e);
                            retry_count += 1;
                            if retry_count >= max_retries {
                                error!("Max Discord reconnection attempts reached. Giving up.");
                                break;
                            }
                            let delay = calculate_backoff(retry_count, base_delay, max_delay);
                            warn!("Retrying in {:?}...", delay);
                            sleep(delay).await;
                            continue;
                        }
                    }
                }
            };

            // Signal ready if this is the first successful connection attempt
            if let Some(tx) = ready_tx.take() {
                let bridge_state = self.config.shared_state.clone();
                tokio::spawn(async move {
                    // Wait for up to 30 seconds for channels to be resolved
                    for _ in 0..60 {
                        {
                            let state = bridge_state.read().await;
                            // Check if we have resolved any channels or if dashboard channel is resolved
                            if !state.wow_to_discord.is_empty() || state.dashboard_channel_id.is_some() {
                                let _ = tx.send(());
                                return;
                            }
                        }
                        sleep(Duration::from_millis(500)).await;
                    }
                    // If timed out, send anyway so we don't hang forever (main loop has its own timeout too)
                    let _ = tx.send(());
                });
            }

            // Run the client
            match client.start().await {
                Ok(()) => {
                    info!("Discord client disconnected normally");
                    break;
                }
                Err(e) => {
                    error!("Discord client error: {}", e);
                    retry_count += 1;

                    if retry_count >= max_retries {
                        error!("Max Discord reconnection attempts ({}) reached. Giving up.", max_retries);
                        break;
                    }

                    let delay = calculate_backoff(retry_count, base_delay, max_delay);
                    warn!(
                        "Discord disconnected. Reconnecting in {:?}... (attempt {}/{})",
                        delay, retry_count + 1, max_retries
                    );
                    sleep(delay).await;

                    // Client is consumed, self.client is already None, will rebuild on next iteration
                }
            }
        }
    }
}

/// Calculate exponential backoff delay with jitter.
fn calculate_backoff(retry_count: u32, base_delay: Duration, max_delay: Duration) -> Duration {
    // Exponential backoff: base * 2^(retry-1), capped at max
    let exp_delay = base_delay.saturating_mul(1 << retry_count.min(6));
    std::cmp::min(exp_delay, max_delay)
}


