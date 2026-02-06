//! Game server connection and protocol handling.

pub mod chat;
pub mod connector;
pub mod guild;
pub mod handler;
pub mod header;
pub mod packets;

pub use connector::new_game_connection;
pub use handler::{ChatProcessingResult, GameHandler};
