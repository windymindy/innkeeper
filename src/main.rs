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
use tracing::{debug, error, info, warn};

use bridge::{BridgeCommand, BridgeMessage, ChannelBundle};
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
    let channels = ChannelBundle::new();

    // Clone senders needed for Discord bot
    let outgoing_wow_tx = channels.game.outgoing_wow_tx.clone();

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
        cmd_response_rx: channels.discord.cmd_response_rx,
        status_rx: channels.discord.status_rx,
        dashboard_rx: channels.discord.dashboard_rx,
        shutdown_rx: channels.game.shutdown_rx.clone(),
    };

    // Create a one-shot channel to signal when Discord initialization is complete
    let (init_complete_tx, init_complete_rx) = tokio::sync::oneshot::channel::<()>();

    let discord_bot = DiscordBotBuilder::new(config.discord.token.clone(), config.clone(), discord_channels, bridge.clone())
        .build(init_complete_tx)
        .await?;

    // ============================================================
    // Spawn forwarding tasks
    // ============================================================

    // Task 1: Game -> Discord forwarding
    let forward_to_discord = {
        let mut game_rx = channels.discord.wow_rx;
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
        let cmd_tx = channels.discord.command_tx;
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
    // Start Discord bot
    // ============================================================
    info!("Starting Discord bot...");

    // We need to wait for Discord to be ready before starting the game client
    // because the game client needs channel mappings resolved from Discord
    // to route messages correctly.

    let discord_task = tokio::spawn(async move {
        discord_bot.run().await;
    });

    info!("Waiting for Discord to connect and resolve channels...");
    // Wait for Discord initialization to complete (or timeout after 15s)
    let discord_init_success = match tokio::time::timeout(tokio::time::Duration::from_secs(15), init_complete_rx).await {
        Ok(Ok(())) => {
            info!("Discord initialization complete! Starting game client...");
            true
        }
        Ok(Err(_)) => {
            error!("Discord init signal sender was dropped before firing - initialization failed");
            false
        }
        Err(_) => {
            error!("Timed out waiting for Discord initialization (15s) - initialization failed");
            false
        }
    };

    if !discord_init_success {
        error!("Failed to initialize Discord client - shutting down");
        // Give a moment for error logs to flush
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        std::process::exit(1);
    }

    // ============================================================
    // Start Game client in separate task
    // ============================================================
    let channels_to_join = bridge.channels_to_join();

    // Prepare for loop
    use backon::BackoffBuilder;
    use common::ActivityStatus;
    use std::time::Duration;

    let realm_host = realm_host.to_string();
    let config_clone = config.clone();

    // Extract shutdown_tx before moving channels.game into the task
    let shutdown_tx = channels.control.shutdown_tx;
    let game_channels = channels.game;

    /// Create an exponential backoff iterator for game reconnection.
    /// 5s initial, 5min max, factor 1.1, with jitter, unlimited retries.
    fn game_backoff() -> impl Iterator<Item = Duration> {
        backon::ExponentialBuilder::default()
            .with_min_delay(Duration::from_secs(5))
            .with_max_delay(Duration::from_mins(5))
            .with_factor(1.1)
            .with_jitter()
            .without_max_times()
            .build()
    }

    let mut game_task = tokio::spawn(async move {
        // Create client once
        let mut game_client = GameClient::new(
            config_clone.clone(),
            game_channels,
            channels_to_join,
        );

        let mut backoff = game_backoff();

        loop {
            // Check for shutdown before connecting
            if game_client.channels.shutdown_rx.has_changed().unwrap_or(false) && *game_client.channels.shutdown_rx.borrow() {
                info!("Shutdown signal detected, stopping reconnection loop");
                break;
            }

            info!("Authenticating with realm server...");
            // Send Connecting status (may fail if shutting down)
            if let Err(e) = game_client.channels.status_tx.send(ActivityStatus::Connecting) {
                debug!("Status channel closed (shutdown in progress): {}", e);
            }

            match connect_and_authenticate(
                &realm_host,
                realm_port,
                &config_clone.wow.account,
                &config_clone.wow.password,
                &config_clone.wow.realm,
            ).await {
                Ok(session) => {
                    info!("Realm authentication successful!");
                    backoff = game_backoff(); // Reset backoff on successful connection

                    // Run game client
                    match game_client.run(session).await {
                        Ok(()) => info!("Game client disconnected"),
                        Err(e) => error!("Game client error: {}", e),
                    }
                }
                Err(e) => {
                    error!("Realm authentication failed: {}", e);
                }
            }

            // We disconnected or failed to connect - notify Discord (may fail if shutting down)
            if let Err(e) = game_client.channels.status_tx.send(ActivityStatus::Disconnected) {
                debug!("Status channel closed: {}", e);
            }
            // Send dashboard offline event
            use common::messages::DashboardEvent;
            if let Err(e) = game_client.channels.dashboard_tx.send(DashboardEvent::SetOffline) {
                debug!("Dashboard channel closed: {}", e);
            }

            // Calculate backoff delay
            let delay = backoff.next().unwrap_or(Duration::from_mins(5));
            info!("Reconnecting in {:.1} seconds...", delay.as_secs_f64());

            // Wait for delay OR shutdown signal
            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                _ = game_client.channels.shutdown_rx.changed() => {
                    if *game_client.channels.shutdown_rx.borrow() {
                        info!("Shutdown signal received during backoff");
                        break;
                    }
                }
            }
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
        // Signal game client to logout (fire-and-forget - if channel closed, client is already gone)
        if let Err(e) = shutdown_tx.send(true) {
            debug!("Shutdown channel closed (game client already exited): {}", e);
        }
        let timeout = tokio::time::Duration::from_secs(5);
        match tokio::time::timeout(timeout, game_task).await {
            Ok(Ok(())) => info!("Game client logged out gracefully"),
            Ok(Err(e)) => warn!("Game client task panicked: {}", e),
            Err(_) => warn!("Game client logout timed out"),
        }
    }

    info!("Exiting...");
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
