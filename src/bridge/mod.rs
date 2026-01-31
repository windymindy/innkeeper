//! Unified bridge module for Discord-WoW message coordination.
//!
//! This module provides a cleaner separation of bridge functionality,
//! consolidating the message flow orchestration, state management,
//! and channel coordination in one place.
//!
//! ## Module Structure
//!
//! - `channels`: Communication channel structures
//! - `orchestrator`: Main bridge orchestrator (`Bridge` struct)
//! - `state`: Shared bridge state (`BridgeState`, `ChannelConfig`)

pub mod channels;
pub mod orchestrator;
pub mod state;

// Re-export main types for convenience
pub use channels::BridgeChannels;
pub use orchestrator::Bridge;
pub use state::{BridgeState, ChannelConfig};

// Re-export message types from common for backwards compatibility
pub use crate::common::{BridgeCommand, IncomingWowMessage, OutgoingWowMessage, WowMessage};
