//! Realm packet handling and Ascension authentication cryptography.

use bytes::{Buf, BufMut, BytesMut};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use hmac::{digest::KeyInit as HmacKeyInit, Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, trace};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::common::error::ProtocolError;
use crate::protocol::realm::packets::{AuthResult, RealmInfo};

type HmacSha256 = Hmac<Sha256>;

// Ascension-specific cryptographic constants
const KEY_CONSTANT_1: [u8; 32] =
    hex_literal::hex!("3642af852369154cfa1145950880108280a4341c26a376431b741e2aae9c2948");
const KEY_CONSTANT_2: [u8; 32] =
    hex_literal::hex!("33ba3128ee614b5845e06b0dad176a9c79344dd7a7a1e2e8d8ad097da9b57f01");
const KEY_CONSTANT_3: [u8; 32] =
    hex_literal::hex!("66d52b01e006cd246f090025d6312c62d13e847c9805956a1c5a10364baa7d82");
const NONCE_CONSTANT_2: [u8; 12] = hex_literal::hex!("9201008ecafa7d60e0acc81e");
const INPUT_CONSTANT_4: [u8; 32] =
    hex_literal::hex!("e815739f8ec810721b93554ca2eac597e05f375261dd72ff30837df951c7a5ed");
const INPUT_CONSTANT_5: [u8; 32] =
    hex_literal::hex!("26986c8a73d24bc41cf386bcb58492416fb579784e1957701a889d97b6550140");
const INPUT_CONSTANT_6: [u8; 2] = hex_literal::hex!("4f4b"); // "OK"

// Version string constant (hex-encoded in original)
const VERSION_STRING: &[u8] =
    b"1|1|DD541D7D87F3A757680395DD1BB309CC8A27D23F695307F3103BD5E283C57C92";

const XOR_MASK: u8 = 0xED;
const HEADER_MAGIC: u32 = 0xFCF4F4E6;

/// Handles realm authentication for Ascension.
pub struct RealmHandler {
    account: String,
    password: String,
    secret_key: StaticSecret,
    public_key: PublicKey,
    key_derived: [u8; 32],
    key_session: [u8; 40],
    proof_2: [u8; 32],
    nonce: [u8; 12],
}

impl RealmHandler {
    /// Create a new realm handler with account credentials.
    pub fn new(account: &str, password: &str) -> Self {
        // Generate random secret key
        let secret_key = StaticSecret::random_from_rng(rand::thread_rng());
        let public_key = PublicKey::from(&secret_key);

        // Calculate shared secret with constant key
        let key_constant_1_pk = PublicKey::from(KEY_CONSTANT_1);
        let shared_secret = secret_key.diffie_hellman(&key_constant_1_pk);

        // Derive keys from shared secret
        let key_derived = Self::derive_key(
            shared_secret.as_bytes(),
            &INPUT_CONSTANT_4,
            &KEY_CONSTANT_3,
            32,
        );
        let key_session_vec = Self::derive_key(
            shared_secret.as_bytes(),
            &INPUT_CONSTANT_5,
            &KEY_CONSTANT_3,
            40,
        );

        let mut key_derived_arr = [0u8; 32];
        key_derived_arr.copy_from_slice(&key_derived[..32]);

        let mut key_session = [0u8; 40];
        key_session.copy_from_slice(&key_session_vec[..40]);

        // Pre-calculate proof
        let proof_2 = Self::hmac_sha256(&key_derived_arr, &INPUT_CONSTANT_6);

        // Generate random nonce
        let mut nonce = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);

        Self {
            account: account.to_uppercase(),
            password: password.to_uppercase(),
            secret_key,
            public_key,
            key_derived: key_derived_arr,
            key_session,
            proof_2,
            nonce,
        }
    }

    /// HKDF-like key derivation used by Ascension.
    fn derive_key(key: &[u8], input1: &[u8], input2: &[u8], size: usize) -> Vec<u8> {
        // Step 1: intermediate = HMAC-SHA256(key, input1)
        let interim = Self::hmac_sha256(key, input1);

        // Step 2: result = HMAC-SHA256(interim, input2 || 0x01)
        let mut data = input2.to_vec();
        data.push(0x01);
        let result1 = Self::hmac_sha256(&interim, &data);

        if size <= 32 {
            return result1[..size].to_vec();
        }

        // Step 3: extend if needed
        let mut data2 = result1.to_vec();
        data2.extend_from_slice(input2);
        data2.push(0x02);
        let result2 = Self::hmac_sha256(&interim, &data2);

        let mut output = result1.to_vec();
        output.extend_from_slice(&result2);
        output.truncate(size);
        output
    }

    /// Compute HMAC-SHA256.
    fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut mac = <HmacSha256 as HmacKeyInit>::new_from_slice(key)
            .expect("HMAC can take key of any size");
        mac.update(data);
        let result = mac.finalize();
        result.into_bytes().into()
    }

    /// Get the session key for game server authentication.
    pub fn session_key(&self) -> [u8; 40] {
        self.key_session
    }

    /// Build the AUTH_LOGON_CHALLENGE packet.
    pub fn build_logon_challenge(&self) -> Result<Vec<u8>, ProtocolError> {
        // Encrypt password
        let password_bytes = self.password.as_bytes();
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key_derived).map_err(|e| {
            ProtocolError::EncryptionError {
                message: e.to_string(),
            }
        })?;

        let nonce = Nonce::from_slice(&self.nonce);
        let encrypted_password =
            cipher
                .encrypt(nonce, password_bytes)
                .map_err(|e| ProtocolError::EncryptionError {
                    message: e.to_string(),
                })?;

        // Split into ciphertext and tag
        let password_len = password_bytes.len();
        let password_ciphertext = &encrypted_password[..password_len];
        let password_tag: [u8; 16] = encrypted_password[password_len..].try_into().unwrap();

        // Build inner payload (data_1)
        let mut data_1 = BytesMut::with_capacity(605 + password_len);

        // Version string (256 bytes, null-padded)
        let mut version_buf = [0u8; 256];
        let version_len = VERSION_STRING.len().min(255);
        version_buf[..version_len].copy_from_slice(&VERSION_STRING[..version_len]);
        data_1.put_slice(&version_buf);

        // Game identifier "WoW" as LE int
        data_1.put_slice(b"WoW\0");

        // Version: 3.3.5
        data_1.put_u8(3); // major
        data_1.put_u8(3); // minor
        data_1.put_u8(5); // patch

        // Build number (12340 for 3.3.5a)
        data_1.put_u16_le(12340);

        // Architecture "x86" as LE int (reversed)
        data_1.put_slice(b"68x\0");

        // Platform "Win" as LE int (reversed)
        data_1.put_slice(b"niW\0");

        // Locale "enUS" as LE int (reversed)
        data_1.put_slice(b"SUne");

        // Timezone offset
        data_1.put_i32_le(180);

        // IP address (127.0.0.1)
        data_1.put_slice(&[127, 0, 0, 1]);

        // Account name (256 bytes, null-padded)
        let mut account_buf = [0u8; 256];
        let account_bytes = self.account.as_bytes();
        let account_len = account_bytes.len().min(255);
        account_buf[..account_len].copy_from_slice(&account_bytes[..account_len]);
        data_1.put_slice(&account_buf);

        // Client public key (32 bytes)
        data_1.put_slice(self.public_key.as_bytes());

        // Nonce (12 bytes)
        data_1.put_slice(&self.nonce);

        // Password tag (16 bytes)
        data_1.put_slice(&password_tag);

        // Password length (4 bytes LE)
        data_1.put_u32_le(password_len as u32);

        // Encrypted password
        data_1.put_slice(password_ciphertext);

        trace!("Inner payload size: {} bytes", data_1.len());

        // Build header (8 bytes)
        let payload_size = data_1.len() + 16; // +16 for outer tag
        let mut header = BytesMut::with_capacity(8);
        header.put_u8(0x00); // CMD_AUTH_LOGON_CHALLENGE
        header.put_u8(8); // Protocol version
        header.put_u16_le(payload_size as u16);
        header.put_u32_le(HEADER_MAGIC);

        // Encrypt payload with AAD (exclude last 4 bytes from encryption)
        let data_to_encrypt = &data_1[..data_1.len() - 4];
        let tail = &data_1[data_1.len() - 4..];

        let cipher2 = ChaCha20Poly1305::new_from_slice(&KEY_CONSTANT_2).map_err(|e| {
            ProtocolError::EncryptionError {
                message: e.to_string(),
            }
        })?;

        let nonce2 = Nonce::from_slice(&NONCE_CONSTANT_2);
        let payload = Payload {
            msg: data_to_encrypt,
            aad: &header[..],
        };

        let encrypted =
            cipher2
                .encrypt(nonce2, payload)
                .map_err(|e| ProtocolError::EncryptionError {
                    message: e.to_string(),
                })?;

        // Split encrypted data and tag
        let encrypted_data = &encrypted[..encrypted.len() - 16];
        let outer_tag: [u8; 16] = encrypted[encrypted.len() - 16..].try_into().unwrap();

        // XOR encrypted data with mask
        let xored_data: Vec<u8> = encrypted_data.iter().map(|b| b ^ XOR_MASK).collect();

        // Build final packet
        let mut packet = Vec::with_capacity(header.len() + 16 + xored_data.len() + 4);
        packet.extend_from_slice(&header);
        packet.extend_from_slice(&outer_tag);
        packet.extend_from_slice(&xored_data);
        packet.extend_from_slice(tail);

        debug!("Built AUTH_LOGON_CHALLENGE packet: {} bytes", packet.len());
        Ok(packet)
    }

    /// Handle AUTH_LOGON_CHALLENGE response from server.
    pub fn handle_logon_challenge_response(&self, data: &[u8]) -> Result<(), ProtocolError> {
        if data.len() < 3 {
            return Err(ProtocolError::PacketTooShort {
                needed: 3,
                got: data.len(),
            });
        }

        let opcode = data[0];
        if opcode != 0x00 {
            return Err(ProtocolError::UnexpectedOpcode {
                expected: 0x00,
                actual: opcode as u16,
            });
        }

        // data[1] is error code (ignored)
        let result = data[2];
        let auth_result = AuthResult::from_code(result);

        if auth_result != AuthResult::Success {
            return Err(ProtocolError::AuthFailed {
                reason: format!("{:?}", auth_result),
            });
        }

        // Check security flag (should be 0, otherwise 2FA required)
        // The security flag is at the end of the packet
        if data.len() >= 118 {
            let security_flag = data[data.len() - 1];
            if security_flag != 0 {
                return Err(ProtocolError::AuthFailed {
                    reason: "Two-factor authentication required".to_string(),
                });
            }
        }

        debug!("Challenge response: success");
        Ok(())
    }

    /// Build AUTH_LOGON_PROOF packet (Ascension sends empty proof).
    pub fn build_logon_proof(&self) -> Vec<u8> {
        let mut packet = Vec::with_capacity(75);
        packet.push(0x01); // CMD_AUTH_LOGON_PROOF
        packet.extend_from_slice(&[0u8; 32]); // A (zeros)
        packet.extend_from_slice(&[0u8; 20]); // M1 (zeros)
        packet.extend_from_slice(&[0u8; 20]); // CRC (zeros)
        packet.push(0); // key_count
        packet.push(0); // security_flags
        packet
    }

    /// Handle AUTH_LOGON_PROOF response from server.
    pub fn handle_logon_proof_response(&self, data: &[u8]) -> Result<(), ProtocolError> {
        if data.len() < 2 {
            return Err(ProtocolError::PacketTooShort {
                needed: 2,
                got: data.len(),
            });
        }

        let opcode = data[0];
        if opcode != 0x01 {
            return Err(ProtocolError::UnexpectedOpcode {
                expected: 0x01,
                actual: opcode as u16,
            });
        }

        let result = data[1];
        let auth_result = AuthResult::from_code(result);

        if auth_result != AuthResult::Success {
            return Err(ProtocolError::AuthFailed {
                reason: format!("{:?}", auth_result),
            });
        }

        // Verify server proof
        if data.len() >= 34 {
            let server_proof: [u8; 32] = data[2..34].try_into().unwrap();
            if server_proof != self.proof_2 {
                return Err(ProtocolError::AuthFailed {
                    reason: "Server proof mismatch".to_string(),
                });
            }
            debug!("Server proof verified");
        }

        debug!("Proof response: success");
        Ok(())
    }

    /// Build REALM_LIST request packet.
    pub fn build_realm_list_request(&self) -> Vec<u8> {
        let mut packet = Vec::with_capacity(5);
        packet.push(0x10); // CMD_REALM_LIST
        packet.extend_from_slice(&[0u8; 4]); // padding
        packet
    }

    /// Handle REALM_LIST response and extract realm info.
    pub fn handle_realm_list_response(&self, data: &[u8]) -> Result<Vec<RealmInfo>, ProtocolError> {
        if data.len() < 7 {
            return Err(ProtocolError::PacketTooShort {
                needed: 7,
                got: data.len(),
            });
        }

        let mut buf = &data[..];

        let opcode = buf.get_u8();
        if opcode != 0x10 {
            return Err(ProtocolError::UnexpectedOpcode {
                expected: 0x10,
                actual: opcode as u16,
            });
        }

        let _size = buf.get_u16_le();
        let _unknown = buf.get_u32_le();
        let realm_count = buf.get_u8();

        debug!("Realm count: {}", realm_count);

        let mut realms = Vec::with_capacity(realm_count as usize);

        for _ in 0..realm_count {
            if buf.remaining() < 7 {
                break;
            }

            let realm_type = buf.get_u32_le();
            let flags = buf.get_u8();

            // Read null-terminated name
            let name = Self::read_cstring(&mut buf)?;

            // Read null-terminated address
            let address = Self::read_cstring(&mut buf)?;

            if buf.remaining() < 7 {
                break;
            }

            let _population = buf.get_f32_le();
            let characters = buf.get_u8();
            let _timezone = buf.get_u8();
            let id = buf.get_u8();

            debug!(
                "Realm: {} at {} (id={}, type={}, flags={})",
                name, address, id, realm_type, flags
            );

            realms.push(RealmInfo {
                id,
                name,
                address,
                realm_type: realm_type as u8,
                flags,
                characters,
            });
        }

        Ok(realms)
    }

    /// Read a null-terminated string from buffer.
    fn read_cstring(buf: &mut &[u8]) -> Result<String, ProtocolError> {
        let mut bytes = Vec::new();
        loop {
            if buf.is_empty() {
                return Err(ProtocolError::InvalidString {
                    message: "Unterminated string".to_string(),
                });
            }
            let b = buf.get_u8();
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        String::from_utf8(bytes).map_err(|e| ProtocolError::InvalidString {
            message: e.to_string(),
        })
    }
}
