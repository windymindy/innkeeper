//! Bridge channel management.
//!
//! Provides communication channel structures for the bridge,
//! grouping related channels for Discord, WoW, and command handling.

use tokio::sync::{mpsc, watch};

use crate::common::messages::DashboardEvent;
use crate::common::{ActivityStatus, BridgeCommand, BridgeMessage};
use crate::discord::commands::CommandResponse;

/// Channels for the game client.
///
/// These are the channels that the GameClient uses to communicate
/// with the rest of the system.
pub struct GameChannels {
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

/// Channels for the Discord handler.
///
/// These are the receivers/senders that the Discord side uses.
pub struct DiscordSideChannels {
    /// Receiver for WoW -> Discord messages.
    pub wow_rx: mpsc::UnboundedReceiver<BridgeMessage>,
    /// Sender for commands (Discord sends commands here).
    pub command_tx: mpsc::UnboundedSender<BridgeCommand>,
    /// Receiver for command responses.
    pub cmd_response_rx: mpsc::UnboundedReceiver<CommandResponse>,
    /// Receiver for status updates.
    pub status_rx: mpsc::UnboundedReceiver<ActivityStatus>,
    /// Receiver for dashboard updates.
    pub dashboard_rx: mpsc::UnboundedReceiver<DashboardEvent>,
}

/// Control channels for shutdown coordination.
pub struct ControlChannels {
    /// Sender to trigger shutdown.
    pub shutdown_tx: watch::Sender<bool>,
}

/// Bundle of all channels created by the bridge.
///
/// This replaces the previous 7-tuple return from `BridgeChannels::new()`.
pub struct ChannelBundle {
    /// Channels for the game client.
    pub game: GameChannels,
    /// Channels for the Discord handler.
    pub discord: DiscordSideChannels,
    /// Control channels for shutdown.
    pub control: ControlChannels,
}

impl ChannelBundle {
    /// Create a new set of bridge channels.
    ///
    /// Returns a structured bundle containing all channels needed
    /// for bidirectional message flow between Discord and WoW.
    pub fn new() -> Self {
        let (wow_tx, wow_rx) = mpsc::unbounded_channel();
        let (outgoing_wow_tx, outgoing_wow_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (command_response_tx, cmd_response_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let (dashboard_tx, dashboard_rx) = mpsc::unbounded_channel();

        Self {
            game: GameChannels {
                wow_tx,
                outgoing_wow_tx,
                outgoing_wow_rx,
                command_rx,
                command_response_tx,
                shutdown_rx,
                status_tx,
                dashboard_tx,
            },
            discord: DiscordSideChannels {
                wow_rx,
                command_tx,
                cmd_response_rx,
                status_rx,
                dashboard_rx,
            },
            control: ControlChannels { shutdown_tx },
        }
    }
}

impl Default for ChannelBundle {
    fn default() -> Self {
        Self::new()
    }
}
