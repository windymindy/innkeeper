//! Realm server connection and authentication.

pub mod connector;
pub mod handler;
pub mod packets;

pub use connector::{connect_and_authenticate, RealmSession};
pub use handler::RealmHandler;
pub use packets::{AuthResult, RealmInfo};
