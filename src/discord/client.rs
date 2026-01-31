//! Discord bot client abstraction.
//!
//! Provides a high-level interface for creating and running the Discord bot,
//! hiding serenity implementation details from the rest of the application.

use std::sync::Arc;
use std::time::Duration;

use serenity::all::Http;
use serenity::Client;
use serenity::model::gateway::GatewayIntents;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::bridge::{BridgeState, ChannelConfig};
use crate::common::{IncomingWowMessage, OutgoingWowMessage};
use crate::config::types::Config;
use crate::game::filter::MessageFilter;
use crate::game::router::parse_channel_config;

use super::commands::WowCommand;
use super::handler::BridgeHandler;
use super::resolver::MessageResolver;

/// Channels for Discord bot communication.
pub struct DiscordChannels {
    /// Sender for outgoing WoW messages (Discord -> WoW).
    pub outgoing_wow_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
    /// Receiver for incoming WoW messages (WoW -> Discord).
    pub wow_to_discord_rx: mpsc::UnboundedReceiver<IncomingWowMessage>,
    /// Sender for commands from Discord.
    pub command_tx: mpsc::UnboundedSender<WowCommand>,
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
}

impl ClientConfig {
    /// Build a new serenity Client with the stored configuration.
    async fn build_client(
        &self,
        wow_rx: mpsc::UnboundedReceiver<IncomingWowMessage>,
        command_tx: mpsc::UnboundedSender<WowCommand>,
    ) -> anyhow::Result<Client> {
        let handler = BridgeHandler::new(wow_rx, command_tx);

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
}

impl DiscordBotBuilder {
    /// Create a new Discord bot builder.
    pub fn new(token: String, config: Config, channels: DiscordChannels) -> Self {
        Self {
            token,
            config,
            channels,
        }
    }

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
                format_wow_to_discord: channel
                    .wow
                    .format
                    .clone()
                    .unwrap_or_else(|| "%user: %message".to_string()),
                format_discord_to_wow: channel
                    .discord
                    .format
                    .clone()
                    .unwrap_or_else(|| "[%user]: %message".to_string()),
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
            resolver: MessageResolver::new(),
            filter: MessageFilter::new(
                self.config.filters.as_ref().and_then(|f| f.patterns.clone()),
                self.config.filters.as_ref().and_then(|f| f.patterns.clone()),
            ),
            pending_channel_configs: pending_configs,
            enable_dot_commands: self.config.discord.enable_dot_commands,
            dot_commands_whitelist: self.config.discord.dot_commands_whitelist.clone(),
            http: None,
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
        };

        // Build initial client
        let handler = BridgeHandler::new(
            self.channels.wow_to_discord_rx,
            self.channels.command_tx.clone(),
        );

        let client = Client::builder(&client_config.token, client_config.intents)
            .event_handler(handler)
            .await?;

        // Store bridge state in client
        {
            let mut data = client.data.write().await;
            data.insert::<BridgeState>(shared_state.clone());
        }

        let http = client.http.clone();

        Ok(DiscordBot {
            client: Some(client),
            http,
            config: client_config,
            outgoing_wow_tx: self.channels.outgoing_wow_tx,
            command_tx: self.channels.command_tx,
        })
    }
}

/// Discord bot instance with reconnection support.
pub struct DiscordBot {
    client: Option<Client>,
    http: Arc<Http>,
    config: ClientConfig,
    // Keep senders to create new receivers for reconnection
    outgoing_wow_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
    command_tx: mpsc::UnboundedSender<WowCommand>,
}

impl DiscordBot {
    /// Get the HTTP client for sending messages.
    pub fn http(&self) -> Arc<Http> {
        self.http.clone()
    }

    /// Run the Discord bot with automatic reconnection.
    pub async fn run(mut self) {
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
                    let (_new_wow_tx, new_wow_rx) = mpsc::unbounded_channel::<IncomingWowMessage>();

                    // Update the shared state with new channel info if needed
                    // The outgoing_wow_tx is already stored in shared state

                    match self.config.build_client(new_wow_rx, self.command_tx.clone()).await {
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

/// Send a command response to a Discord channel.
pub async fn send_command_response(
    http: &Http,
    channel_id: u64,
    content: &str,
) -> anyhow::Result<()> {
    use serenity::model::id::ChannelId;
    let channel = ChannelId::new(channel_id);
    channel.say(http, content).await.map(|_| ())?;
    Ok(())
}
