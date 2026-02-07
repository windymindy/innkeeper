use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use tracing::{debug, info, warn};

use crate::common::{ActivityStatus, BridgeChannels, BridgeCommand, BridgeMessage, CommandResponseData};
use crate::config::types::Config;
use crate::discord::commands::CommandResponse;

use crate::protocol::game::{new_game_connection, GameHandler, ChatProcessingResult};
use crate::protocol::game::chat::chat_events;
use crate::protocol::packets::opcodes::{
    SMSG_AUTH_CHALLENGE, SMSG_AUTH_RESPONSE, SMSG_CHANNEL_NOTIFY, SMSG_CHAR_ENUM, SMSG_CHAT_PLAYER_NOT_FOUND, SMSG_GM_MESSAGECHAT,
    SMSG_GUILD_EVENT, SMSG_GUILD_QUERY, SMSG_GUILD_ROSTER, SMSG_INIT_WORLD_STATES, SMSG_LOGIN_VERIFY_WORLD, SMSG_LOGOUT_COMPLETE,
    SMSG_MESSAGECHAT, SMSG_MOTD, SMSG_NAME_QUERY, SMSG_NOTIFICATION, SMSG_PONG, SMSG_SERVER_MESSAGE, SMSG_UPDATE_OBJECT,
};
use crate::protocol::packets::PacketDecode;
use crate::protocol::realm::connector::RealmSession;
use crate::protocol::game::packets::{AuthChallenge, AuthResponse, CharEnum, InitWorldStates, LoginVerifyWorld, Pong};

pub struct GameClient {
    config: Config,
    session: RealmSession,
    channels: BridgeChannels,
    custom_channels: Vec<String>,
}

impl GameClient {
    pub fn new(config: Config, session: RealmSession, channels: BridgeChannels, custom_channels: Vec<String>) -> Self {
        Self {
            config,
            session,
            channels,
            custom_channels,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let (host, port) = self
            .session
            .realm
            .parse_address()
            .ok_or_else(|| anyhow!("Invalid realm address"))?;
        info!("Connecting to game server at {}:{}", host, port);
        let stream = TcpStream::connect((host, port)).await?;
        self.handle_connection(stream).await
    }

    pub async fn handle_connection<S>(
        &mut self,
        stream: S,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // Send connecting status
        if let Err(e) = self.channels.status_tx.send(ActivityStatus::Connecting) {
            warn!("Failed to send connecting status: {}", e);
        }

        let mut connection = new_game_connection(stream);
        let mut handler = GameHandler::new(
            &self.config.wow.account,
            self.session.session_key,
            self.session.realm.id as u32,
            &self.config.wow.character,
        );
        let mut shutdown_rx = self.channels.shutdown_rx.clone();

        info!("Game connection established");

        loop {
            tokio::select! {
                packet = connection.next() => {
                    match packet {
                        Some(Ok(packet)) => {
                            let mut payload = packet.payload;
                            match packet.opcode {
                                SMSG_AUTH_CHALLENGE => {
                                    let challenge = AuthChallenge::decode(&mut payload)?;
                                    let auth_session = handler.handle_auth_challenge(challenge)?;
                                    connection.send(auth_session.into()).await?;
                                    info!("Sent auth session");
                                    // Initialize header crypt with session key after sending AUTH_SESSION
                                    connection.codec_mut().init_crypt(&self.session.session_key);
                                }
                                SMSG_AUTH_RESPONSE => {
                                    let response = AuthResponse::decode(&mut payload)?;
                                    if handler.handle_auth_response(response)? {
                                        info!("Auth successful, requesting character list");
                                        // Send CMSG_CHAR_ENUM to request character list
                                        let char_enum_req = handler.build_char_enum_request();
                                        connection.send(char_enum_req.into()).await?;
                                    } else {
                                        return Err(anyhow!("Game auth failed"));
                                    }
                                }
                                SMSG_CHAR_ENUM => {
                                    let char_enum = CharEnum::decode(&mut payload)?;
                                    if let Some(char_info) = handler.handle_char_enum(char_enum, &self.config.wow.character) {
                                        let login = handler.build_player_login(char_info.guid);
                                        connection.send(login.into()).await?;
                                        info!("Sent player login for {}", char_info.name);
                                    } else {
                                        return Err(anyhow!("Character '{}' not found", self.config.wow.character));
                                    }
                                }
                                SMSG_LOGIN_VERIFY_WORLD => {
                                    let verify = LoginVerifyWorld::decode(&mut payload)?;
                                    handler.handle_login_verify_world(verify)?;
                                    info!("In world! Starting ping loop and requesting guild info");
                                    
                                    // Send realm status update
                                    if let Err(e) = self.channels.status_tx.send(ActivityStatus::ConnectedToRealm(self.config.wow.realm.clone())) {
                                        warn!("Failed to send realm status: {}", e);
                                    }

                                    // Request guild info if in a guild
                                    if handler.guild_id > 0 {
                                        let guild_query = handler.build_guild_query(handler.guild_id);
                                        connection.send(guild_query.into()).await?;
                                        
                                        let roster_req = handler.request_guild_roster();
                                        connection.send(roster_req.into()).await?;
                                    }
                                    
                                    // Join custom channels
                                    for channel_name in &self.custom_channels {
                                        let join = handler.build_join_channel(channel_name);
                                        connection.send(join.into()).await?;
                                        info!("Joining channel: {}", channel_name);
                                    }
                                }
                                SMSG_MESSAGECHAT => {
                                    match handler.handle_messagechat(payload)? {
                                        Some(ChatProcessingResult::Chat(chat_msg)) => {
                                            // Regular chat message - convert to bridge message
                                            let wow_msg = BridgeMessage {
                                                sender: Some(chat_msg.sender_name),
                                                content: chat_msg.content,
                                                chat_type: chat_msg.chat_type.to_id(),
                                                channel_name: chat_msg.channel_name,
                                                format: chat_msg.format,
                                                guild_event: None,
                                            };

                                            if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                                warn!("Failed to send message to bridge: {}", e);
                                            }
                                        }
                                        Some(ChatProcessingResult::GuildEvent(event_data)) => {
                                            // Send as a guild event BridgeMessage
                                            let wow_msg = BridgeMessage {
                                                sender: Some(event_data.player_name.clone()),
                                                content: String::new(),
                                                chat_type: chat_events::CHAT_MSG_GUILD,
                                                channel_name: None,
                                                format: None,
                                                guild_event: Some(event_data),
                                            };

                                            if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                                warn!("Failed to send message to bridge: {}", e);
                                            }
                                        }
                                        None => {
                                            // Name query needed - check if we have pending messages
                                            if !handler.pending_messages.is_empty() {
                                                // Send name queries for all unknown GUIDs
                                                for guid in handler.pending_messages.keys() {
                                                    let name_query = handler.build_name_query(*guid);
                                                    connection.send(name_query.into()).await?;
                                                }
                                            }
                                        }
                                    }
                                }
                                SMSG_NAME_QUERY => {
                                    let resolved = handler.handle_name_query(payload)?;
                                    for chat_msg in resolved {
                                        // Convert to bridge message
                                        let wow_msg = BridgeMessage {
                                            sender: Some(chat_msg.sender_name),
                                            content: chat_msg.content,
                                            chat_type: chat_msg.chat_type.to_id(),
                                            channel_name: chat_msg.channel_name,
                                            format: chat_msg.format,
                                            guild_event: None,
                                        };

                                        if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                            warn!("Failed to send message to bridge: {}", e);
                                        }
                                    }
                                }
                                SMSG_CHANNEL_NOTIFY => {
                                    handler.handle_channel_notify(payload)?;
                                }
                                SMSG_GUILD_QUERY => {
                                    handler.handle_guild_query(payload)?;
                                }
                                SMSG_GUILD_ROSTER => {
                                    handler.handle_guild_roster(payload)?;
                                    info!("Guild roster loaded: {} members", handler.guild_roster.len());

                                    // Send guild stats update
                                    let online_count = handler.get_online_guildies_count();
                                    if let Err(e) = self.channels.status_tx.send(ActivityStatus::GuildStats { online_count }) {
                                        warn!("Failed to send guild stats status: {}", e);
                                    }
                                }
                                SMSG_GUILD_EVENT => {
                                    if let Some(event_data) = handler.handle_guild_event(payload)? {
                                        // For MOTD events, the target_name contains the MOTD text
                                        // which should go in the content field for %message placeholder
                                        let content = if event_data.event_name == "motd" {
                                            event_data.target_name.clone().unwrap_or_default()
                                        } else {
                                            String::new()
                                        };

                                        info!(
                                            guild_event = %event_data.event_name,
                                            player = %event_data.player_name,
                                            target = ?event_data.target_name,
                                            rank = ?event_data.rank_name,
                                            content = %content,
                                            "Guild event received"
                                        );

                                        // Check if we need to update roster before moving event_data
                                        let should_update_roster = ["online", "offline", "joined", "left", "removed"]
                                            .contains(&event_data.event_name.as_str());
                                        let event_name = event_data.event_name.clone();

                                        // Send guild event as a BridgeMessage to Discord
                                        let wow_msg = BridgeMessage {
                                            sender: Some(event_data.player_name.clone()), // Player who triggered event
                                            content,    // MOTD text goes here for %message placeholder
                                            chat_type: chat_events::CHAT_MSG_GUILD,
                                            channel_name: None,
                                            format: None,
                                            guild_event: Some(event_data), // Contains all event info for formatting
                                        };
                                        if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                            warn!("Failed to send guild event to bridge: {}", e);
                                        }

                                        // Update roster on online/offline/join/left events
                                        if should_update_roster {
                                            let roster_req = handler.request_guild_roster();
                                            if let Err(e) = connection.send(roster_req.into()).await {
                                                warn!("Failed to send guild roster request after event: {}", e);
                                            } else {
                                                debug!("Requested guild roster update after {} event", event_name);
                                            }
                                        }
                                    }
                                }
                                SMSG_NOTIFICATION => {
                                    if let Ok(msg) = handler.handle_notification(payload) {
                                        // Send notification as system message to Discord
                                        let wow_msg = BridgeMessage {
                                               sender: None,
                                               content: msg,
                                               chat_type: chat_events::CHAT_MSG_SYSTEM,
                                               channel_name: None,
                                               format: None,
                                               guild_event: None,
                                        };
                                        if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                            warn!("Failed to send notification to bridge: {}", e);
                                        }
                                    }
                                }
                                SMSG_MOTD => {
                                    if self.config.server_motd_enabled() {
                                        if let Ok(Some(msg)) = handler.handle_motd(payload) {
                                            // Send MOTD as system message to Discord
                                            let wow_msg = BridgeMessage {
                                                sender: None,
                                                content: msg,
                                                chat_type: chat_events::CHAT_MSG_SYSTEM,
                                                channel_name: None,
                                                format: None,
                                                guild_event: None,
                                            };
                                            if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                                warn!("Failed to send MOTD to bridge: {}", e);
                                            }
                                        }
                                    }
                                }
                                SMSG_GM_MESSAGECHAT => {
                                    match handler.handle_gm_messagechat(payload)? {
                                        Some(chat_msg) => {
                                            // Convert to bridge message
                                            let wow_msg = BridgeMessage {
                                                sender: Some(chat_msg.sender_name),
                                                content: chat_msg.content,
                                                chat_type: chat_msg.chat_type.to_id(),
                                                channel_name: chat_msg.channel_name,
                                                format: chat_msg.format,
                                                guild_event: None,
                                            };

                                            if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                                warn!("Failed to send GM message to bridge: {}", e);
                                            }
                                        }
                                        None => {
                                            // Name query needed - check if we have pending messages
                                            if !handler.pending_messages.is_empty() {
                                                // Send name queries for all unknown GUIDs
                                                for guid in handler.pending_messages.keys() {
                                                    let name_query = handler.build_name_query(*guid);
                                                    connection.send(name_query.into()).await?;
                                                }
                                            }
                                        }
                                    }
                                }
                                SMSG_SERVER_MESSAGE => {
                                    if let Ok(msg) = handler.handle_server_message(payload) {
                                        // Send server message as system message to Discord
                                        let wow_msg = BridgeMessage {
                                            sender: None,
                                            content: msg,
                                            chat_type: chat_events::CHAT_MSG_SYSTEM,
                                            channel_name: None,
                                            format: None,
                                            guild_event: None,
                                        };
                                        if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                            warn!("Failed to send server message to bridge: {}", e);
                                        }
                                    }
                                }
                                SMSG_CHAT_PLAYER_NOT_FOUND => {
                                    if let Ok(Some(chat_msg)) = handler.handle_chat_player_not_found(payload) {
                                        // Send "player not found" as WHISPER_INFORM to Discord
                                        let wow_msg = BridgeMessage {
                                            sender: Some(chat_msg.sender_name),
                                            content: chat_msg.content,
                                            chat_type: chat_msg.chat_type.to_id(),
                                            channel_name: chat_msg.channel_name,
                                            format: chat_msg.format,
                                            guild_event: None,
                                        };
                                        if let Err(e) = self.channels.wow_tx.send(wow_msg) {
                                            warn!("Failed to send player not found message to bridge: {}", e);
                                        }
                                    }
                                }
                                SMSG_PONG => {
                                     let pong = Pong::decode(&mut payload)?;
                                     handler.handle_pong(pong);
                                }
                                SMSG_LOGOUT_COMPLETE => {
                                    info!("Logout complete - character logged out gracefully");
                                    return Ok(());
                                }
                                SMSG_INIT_WORLD_STATES => {
                                    let _ = InitWorldStates::decode(&mut payload)?;
                                    handler.handle_init_world_states();
                                }
                                SMSG_UPDATE_OBJECT => {
                                    // SMSG_UPDATE_OBJECT is handled manually with payload bytes
                                    if let Ok(Some(guid)) = handler.handle_update_object(payload, self.config.quirks.sit) {
                                        info!("Found a chair! Sitting on it...");
                                        let interact = handler.build_gameobj_use(guid);
                                        connection.send(interact.into()).await?;
                                    }
                                }
                                _ => {
                                    // Ignore unknown packets
                                }
                            }
                        }
                        Some(Err(e)) => return Err(e.into()),
                        None => return Ok(()), // Connection closed
                    }
                }
                // Shutdown signal received - initiate graceful logout
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        if handler.in_world {
                            info!("Shutdown signal received - logging out character...");
                            let logout_req = handler.build_logout_request();
                            if let Err(e) = connection.send(logout_req.into()).await {
                                warn!("Failed to send logout request: {}", e);
                            }
                            // Wait briefly for logout complete or timeout
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                            info!("Logout timeout or complete - closing connection");
                        }
                        return Ok(());
                    }
                }
                // Ping keepalive every 30 seconds
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    if handler.in_world {
                        let ping = handler.build_ping(0); // sequence doesn't matter for keepalive
                        if let Err(e) = connection.send(ping.into()).await {
                            return Err(anyhow!("Failed to send ping: {}", e));
                        }

                        // Periodic guild roster update (every ~60 seconds)
                        if handler.should_update_guild_roster() {
                            let roster_req = handler.request_guild_roster();
                            if let Err(e) = connection.send(roster_req.into()).await {
                                warn!("Failed to send guild roster request: {}", e);
                            } else {
                                debug!("Requested guild roster update");
                            }
                        }
                    }
                }
                // Outgoing messages from bridge (Discord -> WoW)
                Some(outgoing) = self.channels.outgoing_wow_rx.recv() => {
                    if handler.in_world {
                        let chat_msg = handler.build_chat_message(
                            outgoing.chat_type,
                            &outgoing.content,
                            outgoing.channel_name.as_deref(),
                        );
                        if let Err(e) = connection.send(chat_msg.into()).await {
                            warn!("Failed to send chat message to WoW: {}", e);
                        }
                    }
                }
                // Commands from Discord (!who, !gmotd)
                Some(command) = self.channels.command_rx.recv() => {
                    match command {
                        BridgeCommand::Who { args, reply_channel } => {
                            let content = if let Some(search_name) = args {
                                let member = handler.search_guild_member(&search_name);
                                let guild_name = handler.guild_info.as_ref().map(|g| g.name.clone());
                                CommandResponseData::WhoSearch(search_name, member, guild_name)
                            } else {
                                let members = handler.get_online_guildies();
                                let guild_name = handler.guild_info.as_ref().map(|g| g.name.clone());
                                CommandResponseData::WhoList(members, guild_name)
                            };

                            info!("Processed !who command for channel {}", reply_channel);

                            // Send response back to Discord
                            let cmd_response = CommandResponse {
                                channel_id: reply_channel,
                                content,
                            };
                            if let Err(e) = self.channels.command_response_tx.send(cmd_response) {
                                warn!("Failed to send !who response to bridge: {}", e);
                            }
                        }
                        BridgeCommand::Gmotd { reply_channel } => {
                            let motd = handler.get_guild_motd().map(|s| s.to_string());
                            let guild_name = handler.guild_info.as_ref().map(|g| g.name.clone());
                            let content = CommandResponseData::GuildMotd(motd, guild_name);

                            info!("Processed !gmotd command for channel {}", reply_channel);

                            // Send response back to Discord
                            let cmd_response = CommandResponse {
                                channel_id: reply_channel,
                                content,
                            };
                            if let Err(e) = self.channels.command_response_tx.send(cmd_response) {
                                warn!("Failed to send !gmotd response to bridge: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{
        ChatConfig, DiscordConfig, GuildDashboardConfig, GuildEventsConfig, QuirksConfig, WowConfig,
    };
    use crate::protocol::realm::packets::RealmInfo;

    fn make_test_config() -> Config {
        Config {
            discord: DiscordConfig {
                token: "test".to_string(),
                enable_dot_commands: false,
                dot_commands_whitelist: None,
                enable_commands_channels: None,
                enable_tag_failed_notifications: false,
                enable_markdown: false,
            },
            wow: WowConfig {
                platform: "Mac".to_string(),
                enable_server_motd: false,
                version: "3.3.5".to_string(),
                realm_build: None,
                game_build: None,
                realmlist: "localhost:3724".to_string(),
                realm: "Test".to_string(),
                account: "test".to_string(),
                password: "test".to_string(),
                character: "TestChar".to_string(),
            },
            guild: GuildEventsConfig::default(),
            chat: ChatConfig::default(),
            filters: None,
            guild_dashboard: GuildDashboardConfig::default(),
            quirks: QuirksConfig::default(),
        }
    }

    fn make_test_session() -> RealmSession {
        RealmSession {
            session_key: [0u8; 40],
            realm: RealmInfo {
                id: 1,
                name: "TestRealm".to_string(),
                address: "127.0.0.1:8085".to_string(),
                _realm_type: 0,
                _flags: 0,
                _characters: 0,
            },
        }
    }


    #[tokio::test]
    async fn test_auth_flow() {
        let config = make_test_config();
        let session = make_test_session();
        let (channels, _wow_rx, _cmd_tx, _cmd_resp_rx, _shutdown_tx, _status_tx) = BridgeChannels::new();
        let mut client = GameClient::new(config, session, channels, Vec::new());

        let (client_stream, mut server_stream) = tokio::io::duplex(4096);

        // Spawn client task
        tokio::spawn(async move {
            if let Err(e) = client.handle_connection(client_stream).await {
                // It might fail when we close the stream, which is fine
                println!("Client finished: {:?}", e);
            }
        });

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 1. Server sends SMSG_AUTH_CHALLENGE (0x01EC)
        let mut challenge = Vec::new();
        // Header: Size 10 (8 payload + 2 opcode), Opcode 0x01EC
        challenge.extend_from_slice(&[0x00, 0x0A, 0xEC, 0x01]);
        // Payload: 4 bytes padding + 4 bytes server seed (total 8 bytes)
        challenge.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Padding
        challenge.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // Server Seed
        server_stream.write_all(&challenge).await.unwrap();

        // 2. Expect CMSG_AUTH_SESSION (0x01ED)
        let mut buf = [0u8; 1024];
        let n = server_stream.read(&mut buf).await.unwrap();
        assert!(n > 6);
        // Verify opcode in header (bytes 2-3 of client header)
        // Client header: Size (2 bytes BE), Opcode (4 bytes LE) - wait, encoded as 4 bytes LE?
        // Let's check GamePacketCodec encode:
        // header[2] = opcode_bytes[0]; header[3] = opcode_bytes[1];
        // So bytes 2 and 3 are the opcode (LE).
        let opcode = u16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(opcode, 0x01ED, "Expected CMSG_AUTH_SESSION");

        // 3. Send SMSG_AUTH_RESPONSE (0x01EE) Success
        let mut auth_response = Vec::new();
        // Payload: 1 byte (0x0C = success), + 10 bytes dummy billing/expansion info
        // Total payload: 1 + 4 + 1 + 4 + 1 = 11 bytes?
        // Let's check AuthResponse::Success fields.
        // It likely matches the struct.
        // Assuming simple success payload for now.
        // Opcode: 0x01EE
        // Size: 2 + payload len.
        // We'll construct a minimal success packet.
        // Actually AuthResponse::Success has:
        // billing_time_remaining (u32), billing_flags (u8), billing_time_rested (u32), expansion (u8)
        // Total 10 bytes.
        let payload_len = 1 + 4 + 1 + 4 + 1; // 11 bytes (Code + fields)
        // Header
        auth_response.extend_from_slice(&[0x00, (payload_len as u8) + 2, 0xEE, 0x01]);
        // Code 0x0C (Success) ? No, code is separate in Enum?
        // Check AuthResponse definition. It's an Enum.
        // GameHandler::handle_auth_response takes the Enum.
        // The codec must decode it.
        // Codec uses `Packet` struct with raw payload. `GameHandler` decodes the specific packet struct.
        // So I need to send bytes that `AuthResponse::decode` accepts.
        // AuthResponse::read usually starts with u8 code.
        // Success = 0x0C.
        auth_response.push(0x0C); // Success
        auth_response.extend_from_slice(&[0, 0, 0, 0]); // billing time
        auth_response.push(0); // flags
        auth_response.extend_from_slice(&[0, 0, 0, 0]); // rested
        auth_response.push(2); // expansion (WotLK)
        server_stream.write_all(&auth_response).await.unwrap();

        // 4. Expect CMSG_CHAR_ENUM (0x0037) - client requests character list
        let n = server_stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        let opcode = u16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(opcode, 0x0037, "Expected CMSG_CHAR_ENUM");

        // 5. Send SMSG_CHAR_ENUM (0x003B)
        // We need to send a character list containing "TestChar"
        // Payload: count(u8), [guid(u64), name(cstring), race(u8), class(u8), ...]
        let mut char_enum = Vec::new();
        // Opcode 0x003B
        // We'll construct payload first
        let mut payload = Vec::new();
        payload.push(1); // Count
        // Character 1
        payload.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // GUID 1
        payload.extend_from_slice(b"TestChar\0"); // Name
        payload.push(1); // Race (Human)
        payload.push(1); // Class (Warrior)
        payload.push(1); // Gender
        payload.push(1); // Skin
        payload.push(1); // Face
        payload.push(1); // Hair Style
        payload.push(1); // Hair Color
        payload.push(1); // Facial Hair
        payload.push(80); // Level
        payload.extend_from_slice(&[0, 0, 0, 0]); // Zone
        payload.extend_from_slice(&[0, 0, 0, 0]); // Map
        payload.extend_from_slice(&[0.0_f32.to_bits() as u8, 0, 0, 0]); // X
        payload.extend_from_slice(&[0.0_f32.to_bits() as u8, 0, 0, 0]); // Y
        payload.extend_from_slice(&[0.0_f32.to_bits() as u8, 0, 0, 0]); // Z
        payload.extend_from_slice(&[0, 0, 0, 0]); // Guild ID
        payload.extend_from_slice(&[0, 0, 0, 0]); // Character Flags
        payload.push(0); // Recustomization
        payload.push(0); // First Login
        payload.extend_from_slice(&[0, 0, 0, 0]); // Pet Display ID
        payload.extend_from_slice(&[0, 0, 0, 0]); // Pet Level
        payload.extend_from_slice(&[0, 0, 0, 0]); // Pet Family
        // Equipment (23 slots * (displayid(u32) + inventorytype(u8) + enchant(u32)))
        // 23 * 9 bytes = 207 bytes
        for _ in 0..23 {
             payload.extend_from_slice(&[0, 0, 0, 0]); // Display ID
             payload.push(0); // Inv Type
             payload.extend_from_slice(&[0, 0, 0, 0]); // Enchant
        }

        // Write header
        let size = payload.len() + 2;
        char_enum.push((size >> 8) as u8);
        char_enum.push((size & 0xFF) as u8);
        char_enum.push(0x3B); // Opcode 0x003B
        char_enum.push(0x00);
        char_enum.extend_from_slice(&payload);
        server_stream.write_all(&char_enum).await.unwrap();

        // 6. Expect CMSG_PLAYER_LOGIN (0x003D)
        let n = server_stream.read(&mut buf).await.unwrap();
        assert!(n > 6);
        let opcode = u16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(opcode, 0x003D, "Expected CMSG_PLAYER_LOGIN");
    }
}
