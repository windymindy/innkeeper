//! Bridge orchestrator re-export.
//!
//! This module now re-exports from the unified bridge module.
//! The Bridge struct has been moved to `bridge::orchestrator`.

// Re-export from the new bridge module for backwards compatibility
pub use crate::bridge::Bridge;
pub use crate::bridge::{BridgeChannels, BridgeState, ChannelConfig};
