//! Innkeeper - Discord-WoW chat bridge for Ascension
//!
//! A clientless bot that connects to Ascension WoW server and relays
//! messages between WoW guild/channels and Discord channels.

mod bridge;
mod common;
mod config;
mod discord;
mod game;
mod protocol;

use std::sync::Arc;

use anyhow::Result;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use bridge::{BridgeChannels, BridgeCommand, BridgeMessage};
use config::{load_and_validate, env::get_config_path};
use discord::{
    DiscordBotBuilder, DiscordChannels, WowCommand,
};
use game::GameClient;
use protocol::realm::connector::connect_and_authenticate;

#[tokio::main]
async fn main() -> Result<()> {
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

    let config = load_and_validate(&config_path).map_err(|e| {
        error!("Failed to load configuration: {}", e);
        error!("Please ensure {} exists and is properly formatted.", config_path);
        error!("See the example configuration for reference.");
        e
    })?;

    info!("Configuration loaded successfully");
    info!("  WoW Account: {}", config.wow.account);
    info!("  Character: {}", config.wow.character);
    info!("  Realm: {}", config.wow.realm);
    info!("  Realmlist: {}", config.wow.realmlist);
    info!("  Platform: {}", config.wow.platform);

    // Extract realm host and port from realmlist
    let (realm_host, realm_port) = config.get_realm_host_port();

    // ============================================================
    // Create channels for communication
    // ============================================================

    // Create bridge channels (single source of truth)
    let (game_channels, wow_rx, command_tx, cmd_response_rx, shutdown_tx) = BridgeChannels::new();

    // Clone senders needed for Discord bot
    let outgoing_wow_tx = game_channels.outgoing_wow_tx.clone();

    // WoW -> Discord forwarding channel
    let (wow_to_discord_tx, wow_to_discord_rx) = mpsc::unbounded_channel::<BridgeMessage>();

    // Discord commands channel
    let (discord_command_tx, mut discord_command_rx) = mpsc::unbounded_channel::<WowCommand>();

    // ============================================================
    // Create bridge (for centralized message filtering and routing)
    let bridge = Arc::new(game::Bridge::new(&config));

    // Create Discord bot
    let discord_channels = DiscordChannels {
        outgoing_wow_tx: outgoing_wow_tx.clone(),
        wow_to_discord_rx,
        command_tx: discord_command_tx.clone(),
        cmd_response_rx,
    };

    let discord_bot = DiscordBotBuilder::new(config.discord.token.clone(), config.clone(), discord_channels, bridge.clone())
        .build()
        .await?;

    // ============================================================
    // Spawn forwarding tasks
    // ============================================================

    // Task 1: Game -> Discord forwarding
    let forward_to_discord = {
        let mut game_rx = wow_rx;
        let discord_tx = wow_to_discord_tx;
        tokio::spawn(async move {
            while let Some(msg) = game_rx.recv().await {
                if let Err(e) = discord_tx.send(msg) {
                    error!("Failed to forward message to Discord: {}", e);
                    break;
                }
            }
            info!("Game -> Discord forwarding task ended");
        })
    };

    // Task 2: Command converter (Discord commands -> Game commands)
    let command_converter = {
        let cmd_tx = command_tx;
        tokio::spawn(async move {
            while let Some(cmd) = discord_command_rx.recv().await {
                let bridge_cmd = match cmd {
                    WowCommand::Who { args, reply_channel } => {
                        BridgeCommand::Who { args, reply_channel }
                    }
                    WowCommand::GuildMotd { reply_channel } => {
                        BridgeCommand::Gmotd { reply_channel }
                    }
                };

                if let Err(e) = cmd_tx.send(bridge_cmd) {
                    error!("Failed to forward command: {}", e);
                    break;
                }
            }
            info!("Command converter ended");
        })
    };

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
    // Start game client
    // ============================================================
    let channels_to_join: Vec<String> = bridge
        .channels_to_join()
        .iter()
        .map(|s| s.to_string())
        .collect();

    info!("Starting game client...");
    let mut game_client = GameClient::new(
        config.clone(),
        session,
        game_channels,
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
    // Start Game client in separate task
    // ============================================================
    let mut game_task = tokio::spawn(async move {
        match game_client.run().await {
            Ok(()) => info!("Game client disconnected"),
            Err(e) => error!("Game client error: {}", e),
        }
    });

    // ============================================================
    // Run both clients
    // ============================================================
    let shutdown = tokio::select! {
        biased;
        _ = shutdown_signal() => {
            info!("Shutdown signal received - initiating graceful logout...");
            true
        }
        _ = &mut game_task => false,
        _ = discord_task => false,
        _ = forward_to_discord => false,
        _ = command_converter => false,
    };

    // Handle graceful shutdown
    if shutdown {
        let _ = shutdown_tx.send(true);
        let timeout = tokio::time::Duration::from_secs(5);
        match tokio::time::timeout(timeout, game_task).await {
            Ok(Ok(())) => info!("Game client logged out gracefully"),
            Ok(Err(e)) => warn!("Game client task panicked: {}", e),
            Err(_) => warn!("Game client logout timed out"),
        }
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
