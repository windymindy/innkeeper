//! Innkeeper - Discord-WoW chat bridge for Ascension
//!
//! A clientless bot that connects to Ascension WoW server and relays
//! messages between WoW guild/channels and Discord channels.

mod common;
mod config;
mod discord;
mod game;
mod protocol;

use std::sync::Arc;

use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use config::{load_and_validate, env::get_config_path};
use discord::{
    CommandResponse, DiscordBotBuilder, DiscordChannels, IncomingWowMessage, OutgoingWowMessage,
    WowCommand,
};
use game::bridge::{BridgeChannels, BridgeCommand};
use game::GameClient;
use protocol::realm::connector::connect_and_authenticate;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("Innkeeper v{} starting...", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config_path = get_config_path();
    info!("Loading configuration from {}...", config_path);

    let config = match load_and_validate(&config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            error!("Please ensure {} exists and is properly formatted.", config_path);
            error!("See the example configuration for reference.");
            return Err(e.into());
        }
    };

    info!("Configuration loaded successfully");
    info!("  WoW Account: {}", config.wow.account);
    info!("  Character: {}", config.wow.character);
    info!("  Realm: {}", config.wow.realm);
    info!("  Realmlist: {}", config.wow.realmlist);
    info!("  Platform: {}", config.wow.platform);

    // Extract realm host and port from realmlist
    let (realm_host, realm_port) = config.get_realm_host_port();

    // ============================================================
    // Create channels for Discord <-> WoW communication
    // ============================================================

    // Discord -> Game messages
    let (outgoing_wow_tx, outgoing_wow_rx) = mpsc::unbounded_channel::<OutgoingWowMessage>();

    // Discord commands -> Game commands
    let (discord_command_tx, mut discord_command_rx) = mpsc::unbounded_channel::<WowCommand>();
    let (game_command_tx, game_command_rx) = mpsc::unbounded_channel::<BridgeCommand>();

    // Command responses (Game -> Discord)
    let (cmd_response_tx, cmd_response_rx) = mpsc::unbounded_channel::<CommandResponse>();

    // WoW -> Discord messages
    let (wow_to_discord_tx, wow_to_discord_rx) = mpsc::unbounded_channel::<IncomingWowMessage>();

    // Game client channels
    let (game_bridge_channels, game_wow_rx) = BridgeChannels::new();

    // ============================================================
    // Create Discord bot
    // ============================================================
    let discord_channels = DiscordChannels {
        outgoing_wow_tx: outgoing_wow_tx.clone(),
        wow_to_discord_rx,
        command_tx: discord_command_tx.clone(),
    };

    let discord_bot = DiscordBotBuilder::new(config.discord.token.clone(), config.clone(), discord_channels)
        .build()
        .await?;

    let discord_http = discord_bot.http();

    // ============================================================
    // Spawn forwarding tasks
    // ============================================================

    // Task 1: Game -> Discord
    let forward_to_discord = tokio::spawn(async move {
        let mut game_rx = game_wow_rx;
        let discord_tx = wow_to_discord_tx;

        while let Some(msg) = game_rx.recv().await {
            let incoming = IncomingWowMessage {
                sender: msg.sender,
                content: msg.content,
                chat_type: msg.chat_type,
                channel_name: msg.channel_name,
            };

            if let Err(e) = discord_tx.send(incoming) {
                error!("Failed to forward message to Discord: {}", e);
                break;
            }
        }
        info!("Game->Discord forwarding task ended");
    });

    // Task 2: Command responses -> Discord
    let forward_cmd_responses = tokio::spawn(async move {
        let mut rx = cmd_response_rx;
        while let Some(response) = rx.recv().await {
            if let Err(e) = discord::send_command_response(&discord_http, response.channel_id, &response.content).await {
                error!("Failed to send command response: {}", e);
            }
        }
        info!("Command response task ended");
    });

    // Task 3: Command converter (Discord commands -> Game commands)
    let command_converter = tokio::spawn(async move {
        while let Some(cmd) = discord_command_rx.recv().await {
            let bridge_cmd = match cmd {
                WowCommand::Who { args: _, reply_channel } => {
                    BridgeCommand::Who { reply_channel }
                }
                WowCommand::GuildMotd { reply_channel } => {
                    BridgeCommand::Gmotd { reply_channel }
                }
            };

            if let Err(e) = game_command_tx.send(bridge_cmd) {
                error!("Failed to forward command: {}", e);
                break;
            }
        }
        info!("Command converter ended");
    });

    // ============================================================
    // Connect to WoW realm
    // ============================================================
    info!("Authenticating with realm server...");
    let session = connect_and_authenticate(
        &realm_host,
        realm_port,
        &config.wow.account,
        &config.wow.password,
        &config.wow.realm,
    )
    .await?;

    info!("Realm authentication successful!");

    // ============================================================
    // Create game client channels
    // ============================================================
    let game_bridge_channels = {
        let (game_discord_tx, game_discord_rx) = mpsc::unbounded_channel();
        let (game_cmd_tx, _game_cmd_rx) = mpsc::unbounded_channel::<BridgeCommand>();
        let (_dummy_tx, dummy_rx) = mpsc::unbounded_channel::<CommandResponse>();

        BridgeChannels {
            wow_tx: game_bridge_channels.wow_tx,
            discord_tx: game_discord_tx,
            discord_rx: game_discord_rx,
            outgoing_wow_tx: outgoing_wow_tx.clone(),
            outgoing_wow_rx,
            command_tx: game_cmd_tx,
            command_rx: game_command_rx,
            command_response_tx: cmd_response_tx,
            command_response_rx: dummy_rx,
        }
    };

    // ============================================================
    // Start game client
    // ============================================================
    let bridge = Arc::new(game::Bridge::new(&config));
    let channels_to_join: Vec<String> = bridge
        .channels_to_join()
        .iter()
        .map(|s| s.to_string())
        .collect();

    info!("Starting game client...");
    let mut game_client = GameClient::new(
        config.clone(),
        session,
        game_bridge_channels,
        channels_to_join,
    );

    // ============================================================
    // Start Discord bot
    // ============================================================
    info!("Starting Discord bot...");
    let discord_task = tokio::spawn(async move {
        discord_bot.run().await;
    });

    // ============================================================
    // Run both clients
    // ============================================================
    tokio::select! {
        result = game_client.run() => {
            match result {
                Ok(()) => info!("Game client disconnected"),
                Err(e) => error!("Game client error: {}", e),
            }
        }
        _ = discord_task => warn!("Discord client disconnected"),
        _ = forward_to_discord => warn!("Forwarding ended"),
        _ = forward_cmd_responses => warn!("Command responses ended"),
        _ = command_converter => warn!("Command converter ended"),
        _ = shutdown_signal() => info!("Shutdown signal received"),
    }

    info!("Innkeeper shutting down...");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C"),
        _ = terminate => info!("Received SIGTERM"),
    }
}
