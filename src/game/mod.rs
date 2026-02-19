//! Game logic and World of Warcraft client.
//!
//! This module contains:
//! - Message formatting with placeholder substitution
//! - Game client implementation

pub mod client;
pub mod formatter;

// Re-export commonly used types
pub use client::GameClient;
