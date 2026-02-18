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

    // Extract realm host and port from realmlist
    let (realm_host, realm_port) = config.get_realm_host_port();

    let channels = ChannelBundle::new();
    let outgoing_wow_tx = channels.game.outgoing_wow_tx.clone();
    let (wow_to_discord_tx, wow_to_discord_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    let (discord_command_tx, mut discord_command_rx) = mpsc::unbounded_channel::<WowCommand>();

    let bridge = Arc::new(game::Bridge::new(&config));
    let discord_channels = DiscordChannels {
        outgoing_wow_tx: outgoing_wow_tx.clone(),
        wow_to_discord_rx,
        command_tx: discord_command_tx.clone(),
        cmd_response_rx: channels.discord.cmd_response_rx,
        status_rx: channels.discord.status_rx,
        dashboard_rx: channels.discord.dashboard_rx,
        shutdown_rx: channels.game.shutdown_rx.clone(),
    };

    let (init_complete_tx, init_complete_rx) = tokio::sync::oneshot::channel::<()>();
    let discord_bot = DiscordBotBuilder::new(config.discord.token.clone(), config.clone(), discord_channels, bridge.clone())
        .build(init_complete_tx)
        .await?;

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
            debug!("Game -> Discord forwarding task ended");
        })
    };

    // Task 2: Discord commands -> Bridge commands converter
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

    // Start Discord bot
    info!("Starting Discord bot...");
    let discord_task = tokio::spawn(async move {
        discord_bot.run().await;
    });

    // Wait for Discord initialization
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
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        std::process::exit(1);
    }

    // Game client task
    let channels_to_join = bridge.channels_to_join();
    let realm_host = realm_host.to_string();
    let config_clone = config.clone();
    let shutdown_tx = channels.control.shutdown_tx;
    let mut game_channels = channels.game;

    let mut game_task = tokio::spawn(async move {
        use backon::BackoffBuilder;
        use std::time::Duration;

        fn game_backoff() -> impl Iterator<Item = Duration> {
            backon::ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(5))
                .with_max_delay(Duration::from_mins(5))
                .with_factor(1.1)
                .with_jitter()
                .without_max_times()
                .build()
        }

        let mut backoff = game_backoff();
        let mut connected = false;

        // Clone receivers for continuous reading
        let mut outgoing_rx = game_channels.outgoing_wow_rx;
        let mut command_rx = game_channels.command_rx;
        let cmd_response_tx = game_channels.command_response_tx.clone();

        loop {
            // Check for shutdown
            if game_channels.shutdown_rx.has_changed().unwrap_or(false) && *game_channels.shutdown_rx.borrow() {
                info!("Shutdown signal detected, stopping game task");
                break;
            }

            if connected {
                // This shouldn't happen - connected state is handled inside the block below
                connected = false;
                continue;
            }

            // Not connected - try to authenticate
            info!("Authenticating with realm server...");
            if let Err(e) = game_channels.status_tx.send(common::messages::ActivityStatus::Connecting) {
                debug!("Failed to send status: {}", e);
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
                    backoff = game_backoff();
                    connected = true;

                    // Create game client and run
                    let mut game_client = GameClient::new(
                        config_clone.clone(),
                        bridge::GameChannels {
                            wow_tx: game_channels.wow_tx.clone(),
                            outgoing_wow_tx: game_channels.outgoing_wow_tx.clone(),
                            outgoing_wow_rx: outgoing_rx,
                            command_rx,
                            command_response_tx: game_channels.command_response_tx.clone(),
                            shutdown_rx: game_channels.shutdown_rx.clone(),
                            status_tx: game_channels.status_tx.clone(),
                            dashboard_tx: game_channels.dashboard_tx.clone(),
                        },
                        channels_to_join.clone(),
                    );

                    match game_client.run(session).await {
                        Ok(()) => info!("Game client disconnected"),
                        Err(e) => error!("Game client error: {}", e),
                    }

                    // After disconnect, extract receivers back
                    outgoing_rx = game_client.channels.outgoing_wow_rx;
                    command_rx = game_client.channels.command_rx;
                    connected = false;
                }
                Err(e) => {
                    error!("Realm authentication failed: {}", e);
                }
            }

            if let Err(e) = game_channels.status_tx.send(common::messages::ActivityStatus::Disconnected) {
                debug!("Failed to send status: {}", e);
            }
            if let Err(e) = game_channels.dashboard_tx.send(common::messages::DashboardEvent::SetOffline) {
                debug!("Failed to send dashboard: {}", e);
            }

            // Calculate backoff delay
            let delay = backoff.next().unwrap_or(Duration::from_mins(5));
            info!("Reconnecting in {:.1} seconds...", delay.as_secs_f64());

            // Wait for backoff delay while draining channels
            let sleep = tokio::time::sleep(delay);
            tokio::pin!(sleep);

            loop {
                tokio::select! {
                    // Check if delay is complete - time to reconnect
                    _ = &mut sleep => {
                        break;
                    }
                    // Drain messages silently while waiting
                    _ = outgoing_rx.recv() => {
                        debug!("Dropping message - game disconnected");
                    }
                    // Drain commands with error response while waiting
                    cmd = command_rx.recv() => {
                        match cmd {
                            Some(BridgeCommand::Who { reply_channel, .. }) |
                            Some(BridgeCommand::Gmotd { reply_channel }) => {
                                let error_response = discord::commands::CommandResponse {
                                    channel_id: reply_channel,
                                    content: common::messages::CommandResponseData::Error(
                                        "Not connected to WoW. Please try again later.".to_string()
                                    ),
                                };
                                if let Err(e) = cmd_response_tx.send(error_response) {
                                    warn!("Failed to send command response: {}", e);
                                }
                            }
                            None => {
                                warn!("Command channel closed");
                                return;
                            }
                        }
                    }
                    // Check for shutdown
                    _ = game_channels.shutdown_rx.changed() => {
                        if *game_channels.shutdown_rx.borrow() {
                            return;
                        }
                    }
                }
            }
        }
    });

    // Run all tasks
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

    if shutdown {
        if let Err(e) = shutdown_tx.send(true) {
            debug!("Failed to send shutdown: {}", e);
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
