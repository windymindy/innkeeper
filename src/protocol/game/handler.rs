//! Game packet handling logic.

use sha1::{Digest, Sha1};
use tracing::{debug, error, info};

use crate::common::error::ProtocolError;
use crate::common::types::Player;
use crate::protocol::game::packets::{
    AuthChallenge, AuthResponse, AuthSession, CharEnum, CharacterInfo, LoginVerifyWorld, Ping,
    PlayerLogin, Pong,
};

/// Game protocol handler state.
pub struct GameHandler {
    account: String,
    session_key: [u8; 40],
    realm_id: u32,
    pub player: Option<Player>,
}

impl GameHandler {
    pub fn new(account: &str, session_key: [u8; 40], realm_id: u32) -> Self {
        Self {
            account: account.to_uppercase(),
            session_key,
            realm_id,
            player: None,
        }
    }

    /// Handle SMSG_AUTH_CHALLENGE and build CMSG_AUTH_SESSION.
    pub fn handle_auth_challenge(
        &self,
        packet: AuthChallenge,
    ) -> Result<AuthSession, ProtocolError> {
        let client_seed: u32 = rand::random();
        let server_seed = packet.server_seed;

        // Calculate SHA1 digest
        // SHA1(account + [0,0,0,0] + clientSeed + serverSeed + sessionKey)
        let mut hasher = Sha1::new();
        hasher.update(self.account.as_bytes());
        hasher.update(&[0, 0, 0, 0]);
        hasher.update(&client_seed.to_be_bytes());
        hasher.update(&server_seed.to_be_bytes());
        hasher.update(&self.session_key);
        let digest: [u8; 20] = hasher.finalize().into();

        debug!("Calculated auth digest for account {}", self.account);

        Ok(AuthSession {
            build: 12340, // WotLK 3.3.5a
            login_server_id: 0,
            account: self.account.clone(),
            login_server_type: 0,
            client_seed,
            region_id: 0,
            battlegroup_id: 0,
            realm_id: self.realm_id,
            dos_response: 3,
            digest,
        })
    }

    /// Handle SMSG_AUTH_RESPONSE.
    pub fn handle_auth_response(&self, packet: AuthResponse) -> Result<bool, ProtocolError> {
        match packet {
            AuthResponse::Success {
                billing_time_remaining,
                billing_flags,
                billing_time_rested,
                expansion,
            } => {
                info!(
                    "Game auth successful! Billing: {}/{}/{}, Expansion: {}",
                    billing_time_remaining, billing_flags, billing_time_rested, expansion
                );
                Ok(true)
            }
            AuthResponse::Failure(code) => {
                error!("Game auth failed with code: 0x{:02X}", code);
                Ok(false)
            }
        }
    }

    /// Handle SMSG_CHAR_ENUM.
    pub fn handle_char_enum(
        &mut self,
        packet: CharEnum,
        character_name: &str,
    ) -> Option<CharacterInfo> {
        info!(
            "Received character list with {} characters",
            packet.characters.len()
        );

        for char_info in packet.characters {
            debug!(
                "Character: {} (GUID: {}, Level: {})",
                char_info.name, char_info.guid, char_info.level
            );
            if char_info.name.eq_ignore_ascii_case(character_name) {
                info!("Found character '{}'", char_info.name);
                return Some(char_info);
            }
        }

        None
    }

    /// Handle SMSG_LOGIN_VERIFY_WORLD.
    pub fn handle_login_verify_world(&self, packet: LoginVerifyWorld) -> Result<(), ProtocolError> {
        info!(
            "World login verified! Map: {}, X: {}, Y: {}, Z: {}",
            packet.map_id, packet.x, packet.y, packet.z
        );
        Ok(())
    }

    /// Handle SMSG_PONG (optional, usually just logged).
    pub fn handle_pong(&self, packet: Pong) {
        debug!("Received PONG with sequence: {}", packet.sequence);
    }

    /// Build CMSG_PLAYER_LOGIN.
    pub fn build_player_login(&self, guid: u64) -> PlayerLogin {
        PlayerLogin { guid }
    }

    /// Build CMSG_PING.
    pub fn build_ping(&self, sequence: u32) -> Ping {
        Ping {
            sequence,
            latency: 0,
        }
    }
}
