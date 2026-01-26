//! Game server connection and protocol handling.

pub mod chat;
pub mod connector;
pub mod guild;
pub mod handler;
pub mod header;
pub mod packets;

pub use connector::{new_game_connection, GameConnection};
pub use handler::GameHandler;
pub use packets::{
    AuthChallenge, AuthResponse, AuthSession, CharEnum, CharacterInfo, LoginVerifyWorld, Ping,
    PlayerLogin, Pong,
};

// Re-export chat types
pub use chat::{
    chat_events, chat_notify, languages, ChannelNotify, JoinChannelWotLK, MessageChat, NameQuery,
    NameQueryResponse, SendChatMessage,
};

// Re-export guild types
pub use guild::{
    guild_events, GuildEventPacket, GuildQuery, GuildQueryResponse, GuildRoster, GuildRosterRequest,
};
