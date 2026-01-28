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
use tracing::{error, info, warn};

use config::{load_and_validate, env::apply_env_overrides, validate::validate_config};
use game::{Bridge, BridgeChannels, GameClient};
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
    info!("Loading configuration from innkeeper.conf...");
    let config = match load_and_validate("innkeeper.conf") {
        Ok(mut cfg) => {
            // Apply environment variable overrides
            cfg = apply_env_overrides(cfg);

            // Validate the final config
            validate_config(&cfg)?;
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            error!("Please ensure innkeeper.conf exists and is properly formatted.");
            error!("See the example configuration for reference.");
            return Err(e.into());
        }
    };

    info!("Configuration loaded successfully");
    info!("  WoW Account: {}", config.wow.account.username);
    info!("  Character: {}", config.wow.character);
    info!("  Realm: {} ({}:{})", config.wow.realm.name, config.wow.realm.host, config.wow.realm.port);

    // Connect to realm server and authenticate
    info!("Authenticating with realm server...");
    let session = connect_and_authenticate(
        &config.wow.realm.host,
        config.wow.realm.port,
        &config.wow.account.username,
        &config.wow.account.password,
        &config.wow.realm.name,
    )
    .await?;

    info!("Realm authentication successful!");
    info!("  Session key established");
    info!("  Connecting to game server: {}", session.realm.address);

    // Create bridge for message routing
    let bridge = Arc::new(Bridge::new(&config));
    let channels_to_join: Vec<String> = bridge
        .channels_to_join()
        .iter()
        .map(|s| s.to_string())
        .collect();

    if !channels_to_join.is_empty() {
        info!("Will join custom channels: {}", channels_to_join.join(", "));
    }

    // Create bridge channels for communication
    let bridge_channels = BridgeChannels::new();

    // Start game client
    info!("Starting game client...");
    let mut game_client = GameClient::new(
        config.clone(),
        session,
        bridge_channels,
        channels_to_join,
    );

    // Run the game client with graceful shutdown on Ctrl+C
    tokio::select! {
        result = game_client.run() => {
            match result {
                Ok(()) => {
                    info!("Game client disconnected normally");
                }
                Err(e) => {
                    error!("Game client error: {}", e);
                    return Err(e);
                }
            }
        }
        _ = shutdown_signal() => {
            warn!("Received shutdown signal, disconnecting...");
        }
    }

    info!("Innkeeper shutting down...");
    Ok(())
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM
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
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        }
        _ = terminate => {
            info!("Received SIGTERM signal");
        }
    }
}
