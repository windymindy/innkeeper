//! Realm packet handling and Ascension authentication cryptography.

use bytes::{Buf, BufMut, BytesMut};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
};
use hmac::{digest::KeyInit as HmacKeyInit, Hmac, Mac};
use sha2::Sha256;
use tracing::debug;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::protocol::packets::{read_cstring, MAX_CSTRING_SHORT};
use crate::protocol::realm::packets::{AuthResult, RealmInfo};
use anyhow::{anyhow, Result};

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
const HEADER_MAGIC: u32 = 0xE6F4F4FC;

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
        let secret_key_bytes = {
            let secret_key = StaticSecret::random_from_rng(rand::thread_rng());
            secret_key.to_bytes()
        };

        // Generate random nonce
        let mut nonce = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);

        Self::from_keys(account, password, secret_key_bytes, nonce)
    }

    /// Internal constructor that creates a handler from provided keys.
    /// This is the actual implementation used by both production and tests.
    fn from_keys(
        account: &str,
        password: &str,
        secret_key_bytes: [u8; 32],
        nonce: [u8; 12],
    ) -> Self {
        // Create secret key from bytes
        // Note: For testing, provide pre-clamped keys
        let secret_key = StaticSecret::from(secret_key_bytes);
        let public_key = PublicKey::from(&secret_key);

        // Calculate shared secret with constant key
        let key_constant_1_pk = PublicKey::from(KEY_CONSTANT_1);
        let shared_secret = secret_key.diffie_hellman(&key_constant_1_pk);

        // Derive keys from shared secret
        // Note: Scala signature is derive_key(key, input_1, input_2, size)
        let key_derived = Self::derive_key(
            &KEY_CONSTANT_3,
            shared_secret.as_bytes(),
            &INPUT_CONSTANT_4,
            32,
        );
        let key_session_vec = Self::derive_key(
            &KEY_CONSTANT_3,
            shared_secret.as_bytes(),
            &INPUT_CONSTANT_5,
            40,
        );

        let mut key_derived_arr = [0u8; 32];
        key_derived_arr.copy_from_slice(&key_derived[..32]);

        let mut key_session = [0u8; 40];
        key_session.copy_from_slice(&key_session_vec[..40]);

        // Pre-calculate proof
        let proof_2 = Self::hmac_sha256(&key_derived_arr, &INPUT_CONSTANT_6);

        Self {
            account: account.to_string(),
            password: password.to_string(),
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
    pub fn build_logon_challenge(&self) -> Result<Vec<u8>> {
        // Encrypt password
        let password_bytes = self.password.as_bytes();
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key_derived)
            .map_err(|e| anyhow!("Failed to initialize encryption: {}", e))?;

        let nonce = Nonce::from_slice(&self.nonce);
        let encrypted_password = cipher
            .encrypt(nonce, password_bytes)
            .map_err(|e| anyhow!("Failed to encrypt password: {}", e))?;

        // Split into ciphertext and tag
        let password_len = password_bytes.len();
        let password_ciphertext = &encrypted_password[..password_len];
        let password_tag: [u8; 16] = encrypted_password[password_len..].try_into()?;

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

        // Build header (8 bytes)
        // Payload size = everything after the size field itself:
        //   magic (4) + outer_tag (16) + encrypted_data (data_1.len() - 4) + tail (4)
        //   = 4 + 16 + data_1.len() - 4 + 4 = 20 + data_1.len()
        let payload_size = data_1.len() + 20;
        let mut header = BytesMut::with_capacity(8);
        header.put_u8(0x00); // CMD_AUTH_LOGON_CHALLENGE
        header.put_u8(8); // Protocol version
        header.put_u16_le(payload_size as u16);
        header.put_u32_le(HEADER_MAGIC);

        // Encrypt payload with AAD (exclude last 4 bytes from encryption)
        let data_to_encrypt = &data_1[..data_1.len() - 4];
        let tail = &data_1[data_1.len() - 4..];

        let cipher2 = ChaCha20Poly1305::new_from_slice(&KEY_CONSTANT_2)
            .map_err(|e| anyhow!("Failed to initialize cipher: {}", e))?;

        let nonce2 = Nonce::from_slice(&NONCE_CONSTANT_2);
        let payload = Payload {
            msg: data_to_encrypt,
            aad: &header[..],
        };

        let encrypted = cipher2
            .encrypt(nonce2, payload)
            .map_err(|e| anyhow!("Failed to encrypt data: {}", e))?;

        // Split encrypted data and tag
        let encrypted_data = &encrypted[..encrypted.len() - 16];
        let outer_tag: [u8; 16] = encrypted[encrypted.len() - 16..].try_into()?;

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
    pub fn handle_logon_challenge_response(&self, data: &[u8]) -> Result<()> {
        if data.len() < 3 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                3,
                data.len()
            ));
        }

        let opcode = data[0];
        if opcode != 0x00 {
            return Err(anyhow!(
                "Unexpected opcode: expected 0x00, got 0x{:02X}",
                opcode
            ));
        }

        // data[1] is error code (ignored)
        let result = data[2];
        let auth_result = AuthResult::from_code(result);

        if !auth_result.is_success() {
            return Err(anyhow!("{}", auth_result.get_message()));
        }

        // Check security flag (should be 0, otherwise 2FA required)
        // The security flag is at the end of the packet
        if data.len() >= 118 {
            let security_flag = data[data.len() - 1];
            if security_flag != 0 {
                return Err(anyhow!("Two-factor authentication required"));
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
    pub fn handle_logon_proof_response(&self, data: &[u8]) -> Result<()> {
        if data.len() < 2 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                2,
                data.len()
            ));
        }

        let opcode = data[0];
        if opcode != 0x01 {
            return Err(anyhow!(
                "Unexpected opcode: expected 0x01, got 0x{:02X}",
                opcode
            ));
        }

        let result = data[1];
        let auth_result = AuthResult::from_code(result);

        if !auth_result.is_success() {
            return Err(anyhow!("{}", auth_result.get_message()));
        }

        // Verify server proof
        if data.len() >= 34 {
            let server_proof: [u8; 32] = data[2..34].try_into()?;
            if server_proof != self.proof_2 {
                return Err(anyhow!("Server proof mismatch"));
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
    pub fn handle_realm_list_response(&self, data: &[u8]) -> Result<Vec<RealmInfo>> {
        if data.len() < 7 {
            return Err(anyhow!(
                "Packet too short: need {} bytes, got {}",
                7,
                data.len()
            ));
        }

        let mut buf = &data[..];

        let opcode = buf.get_u8();
        if opcode != 0x10 {
            return Err(anyhow!(
                "Unexpected opcode: expected 0x10, got 0x{:02X}",
                opcode
            ));
        }

        let _size = buf.get_u16_le();
        let _unknown = buf.get_u32_le();
        let realm_count = buf.get_u16_le(); // TBC/WotLK uses u16, not u8

        debug!("Realm count: {}", realm_count);

        let mut realms = Vec::with_capacity(realm_count as usize);

        for _ in 0..realm_count {
            if buf.remaining() < 7 {
                break;
            }

            // TBC/WotLK format: realm_type (1 byte) + lock_flag (1 byte) + flags (1 byte)
            let realm_type = buf.get_u8();
            let _lock_flag = buf.get_u8();
            let flags = buf.get_u8();

            // Read null-terminated name
            let name = read_cstring(&mut buf, MAX_CSTRING_SHORT)?;

            // Read null-terminated address
            let address = read_cstring(&mut buf, MAX_CSTRING_SHORT)?;

            if buf.remaining() < 7 {
                break;
            }

            let _population = buf.get_f32_le();
            let characters = buf.get_u8();
            let _timezone = buf.get_u8();
            let id = buf.get_u8();

            // TBC/WotLK: Skip build information if present (flags & 0x04)
            if (flags & 0x04) == 0x04 {
                if buf.remaining() >= 5 {
                    buf.advance(5); // Skip 5 bytes of build info
                }
            }

            debug!(
                "Realm: {} at {} (id={}, type={}, flags={})",
                name, address, id, realm_type, flags
            );

            realms.push(RealmInfo {
                id,
                name,
                address,
                _realm_type: realm_type,
                _flags: flags,
                _characters: characters,
            });
        }

        Ok(realms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test AUTH_LOGON_CHALLENGE packet generation with known inputs.
    ///
    /// This test uses hardcoded secret key and nonce values to generate a deterministic
    /// AUTH_LOGON_CHALLENGE packet, which can be compared against a known working packet
    /// from a reference client.
    ///
    /// To use this test:
    /// 1. Replace SECRET_KEY and NONCE with values captured from a working client
    /// 2. Update ACCOUNT and PASSWORD to match what the working client used
    /// 3. Run: cargo test -p innkeeper -- --nocapture
    /// 4. Compare the hex output with your working client's packet
    /// 5. Once verified, add the expected packet bytes to EXPECTED_PACKET
    #[test]
    fn test_logon_challenge_with_known_values() {
        // Known inputs - REPLACE THESE WITH YOUR VALUES
        // Note: This key should be clamped (last byte = 0x40)
        const SECRET_KEY: [u8; 32] = hex_literal::hex!(
            "00000000000000000000000000000000"
            "00000000000000000000000000000040"
        );
        const NONCE: [u8; 12] = hex_literal::hex!("000000000000000000000000");
        const ACCOUNT: &str = "testname";
        const PASSWORD: &str = "testpassword";

        // Expected packet - ADD YOUR EXPECTED PACKET HERE AFTER FIRST RUN
        // Leave empty to see what this implementation generates
        const EXPECTED_PACKET: [u8; 641] = hex_literal::hex!("00087d02fcf4f4e61c77e0f6322739d39dde1c5a0d8e789aad7d357018f176dfbea63e0c33295ffe7a3949551f401fc452ef1fd1a837c9558ec8cd8993ac39d2d4a79fd83c07ccba1f5bf36de9f5e20db7a310bd47bed150f3f3e01d836cc89af0f87710248d81daa1b6163ff88d194ae6c7a5856a5cfbcba3f5cd3f644bb97a7f412ceea511d8c58290ed83a41f909a0199e62b285434386b0a408dbf89b731a3404b23d173fee87d6cdd6634cdb0079437bc236cd1db346307b234f8ff6fc697c268de45178f8493f0b60f4ce4998f037f6bb385d178ac9e91dfe40aedc7007064d9d74dba5777776835c5001e774d9767e15cc5f8a2b9d51ac0baf584bf6902a4df54114d87238046d0aee0a32fa155f6868d197968c366cda47f0cfe0034173bb4d9bd170d072a2988b025358b470abae873d55cc31b9bae678fd0ab81c32acb0842b237219774c3a0c3f0c5a7c2c26ddf2c1ff33ffaa4dcaa2163229c2d8687eb5891902b8d5f131bb3c6cecc9c95a158199f8553f22138735ee7a96417a90e0cb7a96fd189aed18ba124720068b8b206f1f0b65d2a397778d15144cd4cf530a16bd2a46dd4e975c48d0ce6488d9bbaedbca73cce9dde67e5d95577c5341fa982a0f726b3fd581e77e355a9670ae88602312b124ecf9caf51a603500b15b8e1daf7c6895d143c56ac7757e8d8e9f258e42d8d0c40782dc714d750f56e36d51e28938921de162cbe7af586fcf32e5bb69f1af225d9b4a375a2ad2a8764bf429251c844710d4cea5a79c0929f25c702817475995cb8e453dcc1ac592aac3279c1fe8b57bca61990828e36dad7ed373e20e5d6cf688bf1974d5b4ef08fdf1714aefdbbd4d279f822b9759480cce5c6c63cd7acb66571d9c2c00ed32af40d1444");

        // Create handler with known values
        let handler = RealmHandler::from_keys(ACCOUNT, PASSWORD, SECRET_KEY, NONCE);

        // Build the logon challenge packet
        let packet = handler
            .build_logon_challenge()
            .expect("Failed to build logon challenge packet");

        // Print packet for comparison
        println!("\n========================================");
        println!("AUTH_LOGON_CHALLENGE Packet ({} bytes)", packet.len());
        println!("========================================");
        print_hex_dump(&packet);
        println!("========================================\n");

        // If expected packet is provided, validate
        if !EXPECTED_PACKET.is_empty() {
            assert_eq!(
                packet.len(),
                EXPECTED_PACKET.len(),
                "Packet length mismatch: got {}, expected {}",
                packet.len(),
                EXPECTED_PACKET.len()
            );

            for (i, (got, expected)) in packet.iter().zip(EXPECTED_PACKET.iter()).enumerate() {
                assert_eq!(
                    got, expected,
                    "Byte mismatch at offset 0x{:04x}: got 0x{:02x}, expected 0x{:02x}",
                    i, got, expected
                );
            }

            println!("✓ Test PASSED - packet matches expected result!");
        } else {
            println!("⚠ No expected packet provided for comparison");
            println!("  Compare the hex dump above with your working client's packet");
            println!("  Then add the expected bytes to EXPECTED_PACKET constant\n");
            panic!("Test incomplete - expected packet not provided for validation");
        }
    }

    /// Helper to print hex dump in a readable format
    fn print_hex_dump(data: &[u8]) {
        for (i, chunk) in data.chunks(16).enumerate() {
            print!("{:04x}: ", i * 16);
            for byte in chunk {
                print!("{:02x} ", byte);
            }
            // Pad if less than 16 bytes
            for _ in 0..(16 - chunk.len()) {
                print!("   ");
            }
            print!(" |");
            for byte in chunk {
                let c = if *byte >= 0x20 && *byte <= 0x7e {
                    *byte as char
                } else {
                    '.'
                };
                print!("{}", c);
            }
            println!("|");
        }
    }
}
