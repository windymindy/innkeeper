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
//! - `state`: Bridge state types (pending, resolved, task contexts)

pub mod channels;
pub mod filter;
pub mod orchestrator;
pub mod state;

// Re-export main types for convenience
pub use channels::{ChannelBundle, GameChannels};
pub use orchestrator::Bridge;
pub use state::{ChannelConfig, PendingBridgeState, ResolvedBridgeState};

// Re-export message types from common for backwards compatibility
pub use crate::common::{BridgeCommand, BridgeMessage};
