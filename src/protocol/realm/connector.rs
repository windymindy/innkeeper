//! Realm server TCP connection and authentication.

use std::net::SocketAddr;

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use anyhow::{anyhow, Context, Result};
use crate::protocol::realm::handler::RealmHandler;
use crate::protocol::realm::packets::RealmInfo;

/// Result of realm authentication.
pub struct RealmSession {
    /// The session key (40 bytes) for game server authentication.
    pub session_key: [u8; 40],
    /// Selected realm information.
    pub realm: RealmInfo,
}

/// Connect to a realm server and authenticate.
pub async fn connect_and_authenticate(
    host: &str,
    port: u16,
    account: &str,
    password: &str,
    realm_name: &str,
) -> Result<RealmSession> {
    let addr = format!("{}:{}", host, port);
    info!("Connecting to realm server at {}", addr);

    let socket_addr: SocketAddr = addr.parse()
        .with_context(|| format!("Invalid address: {}", addr))?;

    let mut stream = TcpStream::connect(socket_addr)
        .await
        .with_context(|| format!("Failed to connect to {}:{}", host, port))?;

    info!("Connected to realm server");

    // Create handler with crypto state
    let handler = RealmHandler::new(account, password);

    // Buffer for reading
    let mut read_buf = BytesMut::with_capacity(4096);

    // Step 1: Send AUTH_LOGON_CHALLENGE
    let challenge_packet = handler.build_logon_challenge()
        .with_context(|| "Failed to build logon challenge")?;

    debug!("Sending AUTH_LOGON_CHALLENGE ({} bytes)", challenge_packet.len());
    stream.write_all(&challenge_packet).await?;

    // Step 2: Read AUTH_LOGON_CHALLENGE response
    read_buf.clear();
    let mut temp_buf = [0u8; 256];
    let n = stream.read(&mut temp_buf).await?;
    if n == 0 {
        return Err(anyhow!("Connection closed by remote"));
    }
    read_buf.extend_from_slice(&temp_buf[..n]);

    debug!("Received {} bytes for challenge response", n);
    handler.handle_logon_challenge_response(&read_buf)
        .map_err(|e| anyhow!("Challenge response failed: {}", e))?;

    // Step 3: Send AUTH_LOGON_PROOF
    let proof_packet = handler.build_logon_proof();
    debug!("Sending AUTH_LOGON_PROOF ({} bytes)", proof_packet.len());
    stream.write_all(&proof_packet).await?;

    // Step 4: Read AUTH_LOGON_PROOF response
    read_buf.clear();
    let n = stream.read(&mut temp_buf).await?;
    if n == 0 {
        return Err(anyhow!("Connection closed by remote"));
    }
    read_buf.extend_from_slice(&temp_buf[..n]);

    debug!("Received {} bytes for proof response", n);
    handler.handle_logon_proof_response(&read_buf)
        .map_err(|e| anyhow!("Proof response failed: {}", e))?;

    info!("Authentication successful");

    // Step 5: Request realm list
    let realm_list_packet = handler.build_realm_list_request();
    debug!("Sending REALM_LIST request");
    stream.write_all(&realm_list_packet).await?;

    // Step 6: Read realm list response
    read_buf.clear();
    // Realm list can be larger, read in chunks
    loop {
        let n = stream.read(&mut temp_buf).await?;
        if n == 0 {
            break;
        }
        read_buf.extend_from_slice(&temp_buf[..n]);
        // Check if we have enough data (realm list has size header)
        if read_buf.len() >= 3 {
            let size = u16::from_le_bytes([read_buf[1], read_buf[2]]) as usize;
            if read_buf.len() >= size + 3 {
                break;
            }
        }
    }

    debug!("Received {} bytes for realm list", read_buf.len());
    let realms = handler.handle_realm_list_response(&read_buf)
        .with_context(|| "Failed to parse realm list")?;

    // Find the requested realm
    let realm = realms
        .into_iter()
        .find(|r| r.name.eq_ignore_ascii_case(realm_name))
        .ok_or_else(|| {
            warn!("Realm '{}' not found", realm_name);
            anyhow!("Realm '{}' not found", realm_name)
        })?;

    info!("Selected realm: {} at {}", realm.name, realm.address);

    Ok(RealmSession {
        session_key: handler.session_key(),
        realm,
    })
}
