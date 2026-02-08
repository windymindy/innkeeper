//! Bridge channel management.
//!
//! Provides communication channel structures for the bridge,
//! grouping related channels for Discord, WoW, and command handling.

use tokio::sync::{mpsc, watch};

use crate::common::messages::DashboardEvent;
use crate::common::{ActivityStatus, BridgeCommand, BridgeMessage};
use crate::discord::commands::CommandResponse;

/// Channels for bridge communication.
///
/// This struct groups all the communication channels needed for
/// bidirectional message flow between Discord and WoW.
pub struct BridgeChannels {
    /// Sender for WoW -> Discord messages.
    pub wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Sender for Discord -> WoW messages (to game handler).
    pub outgoing_wow_tx: mpsc::UnboundedSender<BridgeMessage>,
    /// Receiver for Discord -> WoW messages (game handler listens).
    pub outgoing_wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    /// Receiver for commands (game handler listens).
    pub command_rx: mpsc::UnboundedReceiver<BridgeCommand>,
    /// Sender for command responses (game handler sends).
    pub command_response_tx: mpsc::UnboundedSender<CommandResponse>,
    /// Receiver for shutdown signal (game handler listens).
    pub shutdown_rx: watch::Receiver<bool>,
    /// Sender for status updates (Game -> Discord).
    pub status_tx: mpsc::UnboundedSender<ActivityStatus>,
    /// Sender for dashboard updates (Game -> Discord).
    pub dashboard_tx: mpsc::UnboundedSender<DashboardEvent>,
}

impl BridgeChannels {
    /// Create a new set of bridge channels.
    ///
    /// Returns the channels struct along with:
    /// - wow_rx: Receiver for WoW messages (for forwarding to Discord)
    /// - command_tx: Sender for commands (Discord sends commands here)
    /// - command_response_rx: Receiver for command responses
    /// - shutdown_tx: Sender for shutdown signal (trigger graceful logout)
    /// - status_rx: Receiver for status updates
    /// - dashboard_rx: Receiver for dashboard updates
    pub fn new() -> (
        Self,
        mpsc::UnboundedReceiver<BridgeMessage>,
        mpsc::UnboundedSender<BridgeCommand>,
        mpsc::UnboundedReceiver<CommandResponse>,
        watch::Sender<bool>,
        mpsc::UnboundedReceiver<ActivityStatus>,
        mpsc::UnboundedReceiver<DashboardEvent>,
    ) {
        let (wow_tx, wow_rx) = mpsc::unbounded_channel();
        let (outgoing_wow_tx, outgoing_wow_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (command_response_tx, command_response_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let (dashboard_tx, dashboard_rx) = mpsc::unbounded_channel();

        let channels = Self {
            wow_tx,
            outgoing_wow_tx,
            outgoing_wow_rx,
            command_rx,
            command_response_tx,
            shutdown_rx,
            status_tx,
            dashboard_tx,
        };

        (
            channels,
            wow_rx,
            command_tx,
            command_response_rx,
            shutdown_tx,
            status_rx,
            dashboard_rx,
        )
    }
}

impl Default for BridgeChannels {
    fn default() -> Self {
        let (channels, _, _, _, _, _, _) = Self::new();
        channels
    }
}
