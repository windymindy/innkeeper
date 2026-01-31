//! Game logic and message routing.
//!
//! This module contains:
//! - Message filtering with regex patterns
//! - Message formatting with placeholder substitution
//! - Channel routing between WoW and Discord
//! - The main bridge orchestrator

pub mod bridge;
pub mod client;
pub mod filter;
pub mod formatter;
pub mod router;

// Re-export commonly used types
pub use bridge::Bridge;
pub use client::GameClient;
