//! Innkeeper - Discord-WoW chat bridge for Ascension
//!
//! A clientless bot that connects to Ascension WoW server and relays
//! messages between WoW guild/channels and Discord channels.

mod common;
mod config;
mod discord;
mod game;
mod protocol;

use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("Innkeeper starting...");

    // TODO: Load configuration
    // TODO: Initialize Discord bot
    // TODO: Initialize WoW connection
    // TODO: Start message routing

    Ok(())
}
