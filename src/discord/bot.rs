//! Discord bot setup and connection.
//!
//! Handles connection to Discord, event dispatching, and channel management.

use std::collections::HashMap;
use std::sync::Arc;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::discord::resolver::MessageResolver;

/// Messages sent from WoW to Discord.
#[derive(Debug, Clone)]
pub struct WowToDiscordMessage {
    pub sender: Option<String>,
    pub content: String,
    pub chat_type: u8,
    pub channel_name: Option<String>,
}

/// Messages sent from Discord to WoW.
#[derive(Debug, Clone)]
pub struct DiscordToWowMessage {
    pub sender: String,
    pub content: String,
    pub discord_channel_id: u64,
    pub discord_channel_name: String,
}

/// Channel for receiving Discord -> WoW messages.
pub type DiscordReceiver = mpsc::UnboundedReceiver<DiscordToWowMessage>;

/// Channel for sending WoW -> Discord messages.
pub type WowSender = mpsc::UnboundedSender<WowToDiscordMessage>;

/// Channel mapping configuration.
#[derive(Debug, Clone)]
pub struct ChannelMapping {
    pub discord_channel_id: ChannelId,
    pub discord_channel_name: String,
    pub wow_chat_type: u8,
    pub wow_channel_name: Option<String>,
    pub format: String,
}

/// Shared state for the Discord bot.
pub struct DiscordState {
    /// Mappings from WoW (chat_type, channel_name) -> Discord channels.
    pub wow_to_discord: HashMap<(u8, Option<String>), Vec<ChannelMapping>>,
    /// Mappings from Discord channel ID -> WoW config.
    pub discord_to_wow: HashMap<ChannelId, ChannelMapping>,
    /// Channel to send Discord messages to WoW handler.
    pub discord_tx: mpsc::UnboundedSender<DiscordToWowMessage>,
    /// Message resolver for emoji/mention handling.
    pub resolver: MessageResolver,
}

impl TypeMapKey for DiscordState {
    type Value = Arc<RwLock<DiscordState>>;
}

/// Discord event handler.
pub struct DiscordHandler {
    /// Receiver for WoW -> Discord messages.
    wow_rx: Arc<Mutex<mpsc::UnboundedReceiver<WowToDiscordMessage>>>,
}

impl DiscordHandler {
    pub fn new(wow_rx: mpsc::UnboundedReceiver<WowToDiscordMessage>) -> Self {
        Self {
            wow_rx: Arc::new(Mutex::new(wow_rx)),
        }
    }
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore messages from self
        if msg.author.id == ctx.cache.current_user().id {
            return;
        }

        // Ignore non-text channel messages
        if !msg.guild_id.is_some() {
            return;
        }

        // Ignore bot messages
        if msg.author.bot {
            return;
        }

        let channel_id = msg.channel_id;
        // Get channel name from cache
        let channel_name = if let Some(guild_id) = msg.guild_id {
            if let Some(guild) = ctx.cache.guild(guild_id) {
                guild
                    .channels
                    .get(&msg.channel_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let author_name = msg
            .member
            .as_ref()
            .and_then(|m| m.nick.clone())
            .unwrap_or_else(|| msg.author.name.clone());

        let content = msg.content.clone();

        debug!(
            "Discord message from {} in {}: {}",
            author_name, channel_name, content
        );

        // Check if this is a command
        if content.starts_with('!') {
            if let Err(e) = handle_command(&ctx, &msg, &content).await {
                error!("Command error: {}", e);
            }
            return;
        }

        // Get state and check if this channel is mapped
        let data = ctx.data.read().await;
        if let Some(state) = data.get::<DiscordState>() {
            let state = state.read().await;

            if let Some(mapping) = state.discord_to_wow.get(&channel_id) {
                // Build the message content (include attachments)
                let mut full_content = content;
                for attachment in &msg.attachments {
                    if !full_content.is_empty() {
                        full_content.push(' ');
                    }
                    full_content.push_str(&attachment.url);
                }

                if full_content.is_empty() {
                    warn!("Empty message received from Discord");
                    return;
                }

                let discord_msg = DiscordToWowMessage {
                    sender: author_name,
                    content: full_content,
                    discord_channel_id: channel_id.get(),
                    discord_channel_name: mapping.discord_channel_name.clone(),
                };

                if let Err(e) = state.discord_tx.send(discord_msg) {
                    error!("Failed to send message to WoW handler: {}", e);
                }
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);

        // Start the WoW -> Discord message forwarding task
        let wow_rx = Arc::clone(&self.wow_rx);
        let http = ctx.http.clone();
        let cache = ctx.cache.clone();
        let data = ctx.data.clone();

        tokio::spawn(async move {
            let mut rx = wow_rx.lock().await;
            while let Some(msg) = rx.recv().await {
                let data = data.read().await;
                if let Some(state) = data.get::<DiscordState>() {
                    let state = state.read().await;

                    let key = (msg.chat_type, msg.channel_name.clone().map(|s| s.to_lowercase()));
                    if let Some(mappings) = state.wow_to_discord.get(&key) {
                        for mapping in mappings {
                            // Format the message
                            let formatted = format_wow_message(
                                &mapping.format,
                                msg.sender.as_deref(),
                                &msg.content,
                                msg.channel_name.as_deref(),
                            );

                            // Resolve emojis
                            let resolved = state.resolver.resolve_emojis(&cache, &formatted);

                            // Send to Discord
                            if let Err(e) = mapping.discord_channel_id.say(&http, &resolved).await {
                                error!(
                                    "Failed to send message to Discord channel {}: {}",
                                    mapping.discord_channel_name, e
                                );
                            } else {
                                info!(
                                    "WoW -> Discord ({}): {}",
                                    mapping.discord_channel_name, resolved
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Handle Discord commands (!who, !gmotd, etc.)
async fn handle_command(ctx: &Context, msg: &Message, content: &str) -> Result<(), serenity::Error> {
    let parts: Vec<&str> = content.splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let _args = parts.get(1).copied();

    match command.as_str() {
        "!help" => {
            msg.channel_id
                .say(&ctx.http, "Available commands: `!who`, `!gmotd`, `!help`")
                .await?;
        }
        "!who" | "!gmotd" => {
            // These commands need the WoW handler - send via the channel
            // For now, just acknowledge
            msg.channel_id
                .say(&ctx.http, "Command received - processing...")
                .await?;
        }
        _ => {
            // Unknown command, ignore
        }
    }

    Ok(())
}

/// Format a message from WoW for Discord.
fn format_wow_message(
    format: &str,
    sender: Option<&str>,
    message: &str,
    channel: Option<&str>,
) -> String {
    let time = chrono_time();

    format
        .replace("%time", &time)
        .replace("%user", sender.unwrap_or(""))
        .replace("%message", message)
        .replace("%target", channel.unwrap_or(""))
}

/// Get current time string (HH:MM format).
fn chrono_time() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hours = (now / 3600) % 24;
    let minutes = (now / 60) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

/// Configuration for the Discord bot.
pub struct DiscordBotConfig {
    pub token: String,
    pub channel_mappings: Vec<ChannelMappingConfig>,
}

/// Channel mapping from config.
#[derive(Debug, Clone)]
pub struct ChannelMappingConfig {
    pub discord_channel: String, // Name or ID
    pub wow_chat_type: u8,
    pub wow_channel_name: Option<String>,
    pub format: String,
    pub direction: ChatDirection,
}

/// Direction of chat relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatDirection {
    WowToDiscord,
    DiscordToWow,
    Both,
}

/// Create and run the Discord bot.
pub async fn run_discord_bot(
    config: DiscordBotConfig,
) -> Result<(WowSender, DiscordReceiver), serenity::Error> {
    let (wow_tx, wow_rx) = mpsc::unbounded_channel::<WowToDiscordMessage>();
    let (discord_tx, discord_rx) = mpsc::unbounded_channel::<DiscordToWowMessage>();

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS;

    let handler = DiscordHandler::new(wow_rx);

    let mut client = Client::builder(&config.token, intents)
        .event_handler(handler)
        .await?;

    // Initialize state (will be populated on ready with actual channel IDs)
    let state = DiscordState {
        wow_to_discord: HashMap::new(),
        discord_to_wow: HashMap::new(),
        discord_tx,
        resolver: MessageResolver::new(),
    };

    {
        let mut data = client.data.write().await;
        data.insert::<DiscordState>(Arc::new(RwLock::new(state)));
    }

    // Spawn the client
    tokio::spawn(async move {
        if let Err(e) = client.start().await {
            error!("Discord client error: {:?}", e);
        }
    });

    Ok((wow_tx, discord_rx))
}
