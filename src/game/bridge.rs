//! Bridge orchestrator that ties WoW and Discord together.
//!
//! Manages the bidirectional message flow, filtering, formatting, and routing.

use std::sync::Arc;

use serenity::all::{ChannelId, Http};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::config::types::Config;
use crate::discord::resolver::MessageResolver;
use crate::game::filter::MessageFilter;
use crate::game::formatter::{escape_discord_markdown, split_message, FormatContext, MessageFormatter};
use crate::game::router::{MessageRouter, SharedRouter};

/// Message from WoW to be sent to Discord.
#[derive(Debug, Clone)]
pub struct WowMessage {
    /// Sender's name (None for system messages).
    pub sender: Option<String>,
    /// Message content.
    pub content: String,
    /// WoW chat type.
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
    /// Custom format override.
    pub format: Option<String>,
}

/// Message from Discord to be sent to WoW.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    /// Sender's Discord display name.
    pub sender: String,
    /// Message content.
    pub content: String,
    /// Discord channel ID.
    pub channel_id: u64,
    /// Discord channel name.
    pub channel_name: String,
}

/// Command request from Discord.
#[derive(Debug, Clone)]
pub enum BridgeCommand {
    /// Request guild roster (online guildies).
    Who { reply_channel: u64 },
    /// Request guild MOTD.
    Gmotd { reply_channel: u64 },
}

/// Command response to be sent to Discord.
#[derive(Debug, Clone)]
pub struct CommandResponse {
    /// The Discord channel ID to send the response to.
    pub channel_id: u64,
    /// The response content.
    pub content: String,
}

/// Message to send to WoW game handler.
#[derive(Debug, Clone)]
pub struct OutgoingWowMessage {
    /// Chat type to send as.
    pub chat_type: u8,
    /// Channel name for custom channels.
    pub channel_name: Option<String>,
    /// Sender's name (for formatting).
    pub sender: String,
    /// Message content.
    pub content: String,
}

/// Channels for bridge communication.
pub struct BridgeChannels {
    /// Sender for WoW -> Discord messages.
    pub wow_tx: mpsc::UnboundedSender<WowMessage>,
    /// Receiver for WoW -> Discord messages.
    pub wow_rx: mpsc::UnboundedReceiver<WowMessage>,
    /// Sender for Discord -> WoW messages.
    pub discord_tx: mpsc::UnboundedSender<DiscordMessage>,
    /// Receiver for Discord -> WoW messages.
    pub discord_rx: mpsc::UnboundedReceiver<DiscordMessage>,
    /// Sender for outgoing WoW messages (to game handler).
    pub outgoing_wow_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
    /// Receiver for outgoing WoW messages (game handler listens).
    pub outgoing_wow_rx: mpsc::UnboundedReceiver<OutgoingWowMessage>,
    /// Sender for commands.
    pub command_tx: mpsc::UnboundedSender<BridgeCommand>,
    /// Receiver for commands (game handler listens).
    pub command_rx: mpsc::UnboundedReceiver<BridgeCommand>,
    /// Sender for command responses (game handler sends, bridge receives).
    pub command_response_tx: mpsc::UnboundedSender<CommandResponse>,
    /// Receiver for command responses (bridge listens).
    pub command_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
}

impl BridgeChannels {
    /// Create a new set of bridge channels.
    pub fn new() -> Self {
        let (wow_tx, wow_rx) = mpsc::unbounded_channel();
        let (discord_tx, discord_rx) = mpsc::unbounded_channel();
        let (outgoing_wow_tx, outgoing_wow_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (command_response_tx, command_response_rx) = mpsc::unbounded_channel();

        Self {
            wow_tx,
            wow_rx,
            discord_tx,
            discord_rx,
            outgoing_wow_tx,
            outgoing_wow_rx,
            command_tx,
            command_rx,
            command_response_tx,
            command_response_rx,
        }
    }
}

impl Default for BridgeChannels {
    fn default() -> Self {
        Self::new()
    }
}

/// Discord channel resolver function type.
pub type ChannelResolver = Arc<dyn Fn(&str) -> Option<ChannelId> + Send + Sync>;

/// The main bridge that orchestrates message flow.
pub struct Bridge {
    /// Message router.
    router: SharedRouter,
    /// Message filter.
    filter: Arc<MessageFilter>,
    /// Message resolver for Discord transformations.
    resolver: Arc<MessageResolver>,
    /// Whether dot commands are enabled.
    enable_dot_commands: bool,
    /// Channel name resolver (set by Discord module).
    channel_resolver: Option<ChannelResolver>,
}

impl Bridge {
    /// Create a new bridge from configuration.
    pub fn new(config: &Config) -> Self {
        let router = if config.chat.channels.is_empty() {
            Arc::new(MessageRouter::empty())
        } else {
            Arc::new(MessageRouter::from_config(&config.chat))
        };

        let filter = if let Some(ref filters) = config.filters {
            let enabled = filters.enabled;
            if enabled {
                Arc::new(MessageFilter::new(
                    filters.patterns.clone(),
                    filters.patterns.clone(),
                ))
            } else {
                Arc::new(MessageFilter::empty())
            }
        } else {
            Arc::new(MessageFilter::empty())
        };

        let enable_dot_commands = config.discord.enable_dot_commands;

        Self {
            router,
            filter,
            resolver: Arc::new(MessageResolver::new()),
            enable_dot_commands,
            channel_resolver: None,
        }
    }

    /// Set the channel resolver for looking up Discord channel IDs by name.
    pub fn set_channel_resolver(&mut self, resolver: ChannelResolver) {
        self.channel_resolver = Some(resolver);
    }

    /// Get the router for external use.
    pub fn router(&self) -> SharedRouter {
        Arc::clone(&self.router)
    }

    /// Get the list of custom channels to join in WoW.
    pub fn channels_to_join(&self) -> Vec<&str> {
        self.router.get_channels_to_join()
    }

    /// Process a message from WoW and send to Discord.
    pub async fn handle_wow_to_discord(
        &self,
        msg: WowMessage,
        http: &Http,
    ) {
        let routes = self.router.get_discord_targets(
            msg.chat_type,
            msg.channel_name.as_deref(),
        );

        if routes.is_empty() {
            debug!(
                chat_type = msg.chat_type,
                channel = ?msg.channel_name,
                "No Discord route for WoW message"
            );
            return;
        }

        // Pre-process the message: resolve links, strip colors, etc.
        let processed = self.resolver.resolve_links(&msg.content);
        let processed = self.resolver.strip_color_coding(&processed);
        let processed = self.resolver.strip_texture_coding(&processed);
        // Escape Discord markdown (`, *, _, ~)
        let processed = if msg.sender.is_some() {
            escape_discord_markdown(&processed)
        } else {
            processed
        };

        for route in routes {
            // Resolve Discord channel name to ID
            let channel_id = if let Some(ref resolver) = self.channel_resolver {
                match resolver(&route.discord_channel_name) {
                    Some(id) => id,
                    None => {
                        warn!(
                            channel_name = route.discord_channel_name,
                            "Could not resolve Discord channel name to ID"
                        );
                        continue;
                    }
                }
            } else {
                warn!("No channel resolver set, cannot send to Discord");
                continue;
            };

            // Use route's format or message's format override
            let format = msg.format.as_ref()
                .or(route.wow_to_discord_format.as_ref())
                .cloned()
                .unwrap_or_else(|| "[%user]: %message".to_string());

            let formatter = MessageFormatter::new(&format);
            let ctx = FormatContext::new(
                msg.sender.as_deref().unwrap_or(""),
                &processed,
            )
            .with_channel(msg.channel_name.as_deref().unwrap_or(""));

            let formatted = formatter.format(&ctx);

            // Apply filter
            if self.filter.should_filter_wow_to_discord(&formatted) {
                info!(
                    channel_name = route.discord_channel_name,
                    "FILTERED WoW->Discord: {}",
                    formatted
                );
                continue;
            }

            info!(
                channel_name = route.discord_channel_name,
                "WoW->Discord: {}",
                formatted
            );

            if let Err(e) = channel_id.say(http, &formatted).await {
                error!(
                    channel_name = route.discord_channel_name,
                    error = %e,
                    "Failed to send message to Discord"
                );
            }
        }
    }

    /// Process a message from Discord and prepare for WoW.
    ///
    /// Returns the messages to send to WoW, already formatted and split if needed.
    pub fn handle_discord_to_wow(
        &self,
        msg: &DiscordMessage,
    ) -> Vec<OutgoingWowMessage> {
        let routes = self.router.get_wow_targets(&msg.channel_name);

        if routes.is_empty() {
            debug!(
                channel_name = msg.channel_name,
                "No WoW route for Discord message"
            );
            return Vec::new();
        }

        let mut results = Vec::new();

        for route in routes {
            // Check for dot commands: messages starting with "." that should be sent directly
            let is_dot_command = self.enable_dot_commands && msg.content.starts_with('.');
            
            if is_dot_command {
                // Send the content directly without formatting
                results.push(OutgoingWowMessage {
                    chat_type: route.wow_channel.to_chat_type(),
                    channel_name: route.wow_channel.channel_name().map(|s| s.to_string()),
                    sender: msg.sender.clone(),
                    content: msg.content.clone(),
                });
                continue;
            }

            // Get format and create formatter
            let format = route.discord_to_wow_format.as_ref()
                .cloned()
                .unwrap_or_else(|| "%user: %message".to_string());
            
            let formatter = MessageFormatter::new(&format);
            
            // Calculate max message length and split if needed
            let max_len = formatter.max_message_length(&msg.sender, 255);
            let chunks = split_message(&msg.content, max_len);

            for chunk in chunks {
                let ctx = FormatContext::new(&msg.sender, &chunk);
                let formatted = formatter.format(&ctx);

                // Apply filter
                if self.filter.should_filter_discord_to_wow(&formatted) {
                    info!(
                        wow_channel = ?route.wow_channel,
                        "FILTERED Discord->WoW: {}",
                        formatted
                    );
                    continue;
                }

                info!(
                    wow_channel = ?route.wow_channel,
                    "Discord->WoW: {}",
                    formatted
                );

                results.push(OutgoingWowMessage {
                    chat_type: route.wow_channel.to_chat_type(),
                    channel_name: route.wow_channel.channel_name().map(|s| s.to_string()),
                    sender: msg.sender.clone(),
                    content: formatted,
                });
            }
        }

        results
    }
}

/// Run the WoW -> Discord message forwarding loop.
pub async fn run_wow_to_discord_loop(
    bridge: Arc<Bridge>,
    mut wow_rx: mpsc::UnboundedReceiver<WowMessage>,
    http: Arc<Http>,
) {
    info!("Starting WoW -> Discord message loop");

    while let Some(msg) = wow_rx.recv().await {
        bridge.handle_wow_to_discord(msg, &http).await;
    }

    warn!("WoW -> Discord message loop ended");
}

/// Run the Discord -> WoW message forwarding loop.
pub async fn run_discord_to_wow_loop(
    bridge: Arc<Bridge>,
    mut discord_rx: mpsc::UnboundedReceiver<DiscordMessage>,
    outgoing_tx: mpsc::UnboundedSender<OutgoingWowMessage>,
) {
    info!("Starting Discord -> WoW message loop");

    while let Some(msg) = discord_rx.recv().await {
        let messages = bridge.handle_discord_to_wow(&msg);
        for outgoing in messages {
            if outgoing_tx.send(outgoing).is_err() {
                error!("Failed to send message to WoW handler - channel closed");
                return;
            }
        }
    }

    warn!("Discord -> WoW message loop ended");
}

/// Run the command response loop (sends command responses to Discord).
pub async fn run_command_response_loop(
    mut response_rx: mpsc::UnboundedReceiver<CommandResponse>,
    http: Arc<Http>,
) {
    info!("Starting command response loop");

    while let Some(response) = response_rx.recv().await {
        let channel_id = ChannelId::new(response.channel_id);
        
        if let Err(e) = channel_id.say(&http, &response.content).await {
            error!(
                channel_id = response.channel_id,
                error = %e,
                "Failed to send command response to Discord"
            );
        }
    }

    warn!("Command response loop ended");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{
        ChannelMapping, ChatConfig, DiscordChannelConfig, DiscordConfig,
        GuildDashboardConfig, GuildEventsConfig, QuirksConfig, WowChannelConfig, WowConfig,
    };

    fn make_test_config() -> Config {
        Config {
            discord: DiscordConfig {
                token: Some("test".to_string()),
                enable_dot_commands: true,
                dot_commands_whitelist: None,
                enable_commands_channels: None,
                enable_tag_failed_notifications: false,
            },
            wow: WowConfig {
                platform: "Mac".to_string(),
                enable_server_motd: false,
                version: "3.3.5".to_string(),
                realm_build: None,
                game_build: None,
                realmlist: "localhost:3724".to_string(),
                realm: "Test".to_string(),
                account: "test".to_string(),
                password: "test".to_string(),
                character: "TestChar".to_string(),
            },
            guild: GuildEventsConfig::default(),
            chat: ChatConfig {
                channels: vec![
                    ChannelMapping {
                        direction: "both".to_string(),
                        wow: WowChannelConfig {
                            channel_type: "Guild".to_string(),
                            channel: None,
                            format: Some("[%user]: %message".to_string()),
                        },
                        discord: DiscordChannelConfig {
                            channel: "guild-chat".to_string(),
                            format: Some("[%user]: %message".to_string()),
                        },
                    },
                ],
            },
            filters: None,
            guild_dashboard: GuildDashboardConfig::default(),
            quirks: QuirksConfig::default(),
        }
    }

    #[test]
    fn test_bridge_creation() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        assert!(bridge.enable_dot_commands);
        assert!(bridge.channels_to_join().is_empty()); // "guild" is not a custom channel
    }

    #[test]
    fn test_discord_to_wow_processing() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        let msg = DiscordMessage {
            sender: "Player".to_string(),
            content: "Hello world!".to_string(),
            channel_id: 123456789,
            channel_name: "guild-chat".to_string(),
        };

        let results = bridge.handle_discord_to_wow(&msg);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "[Player]: Hello world!");
    }

    #[test]
    fn test_dot_command_passthrough() {
        let config = make_test_config();
        let bridge = Bridge::new(&config);

        let msg = DiscordMessage {
            sender: "Player".to_string(),
            content: ".help".to_string(),
            channel_id: 123456789,
            channel_name: "guild-chat".to_string(),
        };

        let results = bridge.handle_discord_to_wow(&msg);
        assert_eq!(results.len(), 1);
        // Dot commands are sent directly without formatting
        assert_eq!(results[0].content, ".help");
    }
}
