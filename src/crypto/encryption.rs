use crate::error::ApiSnapError;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

pub const NONCE_LEN: usize = 12;
pub const KEY_LEN: usize = 32;

/// Authenticated AES-256-GCM encryption manager for snapshot store at-rest encryption.
#[derive(Clone)]
pub struct SnapshotEncryptor {
    cipher: Aes256Gcm,
}

impl SnapshotEncryptor {
    /// Initialize with a 32-byte (256-bit) raw master key.
    pub fn new(key_bytes: &[u8; KEY_LEN]) -> Self {
        let cipher = Aes256Gcm::new_from_slice(key_bytes).expect("valid 32-byte key");
        Self { cipher }
    }

    /// Construct from a 64-character hex string or 32-character UTF-8 string.
    pub fn from_key_str(key_str: &str) -> Result<Self, ApiSnapError> {
        let trimmed = key_str.trim();
        let mut key_buf = [0u8; KEY_LEN];

        if trimmed.len() == 64 {
            if let Ok(decoded) = hex::decode(trimmed) {
                if decoded.len() == KEY_LEN {
                    key_buf.copy_from_slice(&decoded);
                    return Ok(Self::new(&key_buf));
                }
            }
        }

        let bytes = trimmed.as_bytes();
        if bytes.len() >= KEY_LEN {
            key_buf.copy_from_slice(&bytes[..KEY_LEN]);
            Ok(Self::new(&key_buf))
        } else {
            Err(ApiSnapError::InvalidConfig {
                location: "encryption.master_key".into(),
                reason: "master key must be at least 32 bytes or 64 hex characters".into(),
            })
        }
    }

    /// Try to construct from `APISNAP_MASTER_KEY` environment variable.
    pub fn from_env() -> Option<Result<Self, ApiSnapError>> {
        std::env::var("APISNAP_MASTER_KEY").ok().map(|k| Self::from_key_str(&k))
    }

    /// Encrypt plaintext bytes using AES-256-GCM.
    /// Layout: `[12-byte Nonce] + [Ciphertext with 16-byte Poly1305 Auth Tag]`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, ApiSnapError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| ApiSnapError::InvalidConfig {
                location: "crypto.encrypt".into(),
                reason: format!("AES-GCM encryption failed: {e}"),
            })?;

        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Decrypt ciphertext bytes using AES-256-GCM and verify integrity tag.
    pub fn decrypt(&self, payload: &[u8]) -> Result<Vec<u8>, ApiSnapError> {
        if payload.len() < NONCE_LEN {
            return Err(ApiSnapError::InvalidConfig {
                location: "crypto.decrypt".into(),
                reason: "corrupted encrypted payload: buffer shorter than nonce length".into(),
            });
        }

        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| ApiSnapError::InvalidConfig {
                location: "crypto.decrypt".into(),
                reason: format!("AES-GCM authentication/decryption failed (invalid key or tampered payload): {e}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_gcm_roundtrip() {
        let key = [0x42u8; KEY_LEN];
        let encryptor = SnapshotEncryptor::new(&key);

        let plaintext = b"{\"secret_token\": \"top_secret_12345\", \"user\": \"admin\"}";
        let encrypted = encryptor.encrypt(plaintext).unwrap();

        assert_ne!(encrypted, plaintext);
        assert!(encrypted.len() > plaintext.len());

        let decrypted = encryptor.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_tamper_detection() {
        let key = [0x77u8; KEY_LEN];
        let encryptor = SnapshotEncryptor::new(&key);

        let mut encrypted = encryptor.encrypt(b"unaltered payload").unwrap();
        // Tamper with last byte
        let last_idx = encrypted.len() - 1;
        encrypted[last_idx] ^= 0xFF;

        let result = encryptor.decrypt(&encrypted);
        assert!(result.is_err(), "Tampered ciphertext must fail authentication check");
    }
}
