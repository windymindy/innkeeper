//! Game logic and message routing.
//!
//! This module contains:
//! - Message formatting with placeholder substitution
//! - Channel routing between WoW and Discord

pub mod client;
pub mod formatter;
pub mod router;

// Re-export commonly used types
pub use client::GameClient;

// Re-export bridge orchestrator for backwards compatibility
pub use crate::bridge::Bridge;
