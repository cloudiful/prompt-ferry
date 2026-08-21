use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use rand::Rng;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const KEY_VERSION_V1: i16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecretEnvelope {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub key_version: i16,
}

#[derive(Debug, Clone)]
pub struct RelaySecretManager {
    key: [u8; KEY_BYTES],
}

impl RelaySecretManager {
    pub fn from_base64(value: &str) -> anyhow::Result<Self> {
        let value = value.trim();
        if value.is_empty() {
            return Err(anyhow!(
                "relay secret master key is required for encrypted secret storage"
            ));
        }
        let bytes = STANDARD
            .decode(value.as_bytes())
            .context("relay secret master key must be valid base64")?;
        let key = bytes.try_into().map_err(|bytes: Vec<u8>| {
            anyhow!(
                "relay secret master key must decode to {KEY_BYTES} bytes, got {}",
                bytes.len()
            )
        })?;
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<EncryptedSecretEnvelope> {
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let mut nonce = [0_u8; NONCE_BYTES];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(&Nonce::from(nonce), plaintext.as_bytes())
            .map_err(|_| anyhow!("failed to encrypt relay secret"))?;
        Ok(EncryptedSecretEnvelope {
            ciphertext,
            nonce: nonce.to_vec(),
            key_version: KEY_VERSION_V1,
        })
    }

    pub fn decrypt(&self, envelope: &EncryptedSecretEnvelope) -> anyhow::Result<String> {
        if envelope.key_version != KEY_VERSION_V1 {
            return Err(anyhow!(
                "unsupported relay secret key version {}",
                envelope.key_version
            ));
        }
        let nonce: &[u8; NONCE_BYTES] = envelope
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("relay secret nonce must be {NONCE_BYTES} bytes"))?;
        let nonce = Nonce::from(*nonce);
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let plaintext = cipher
            .decrypt(&nonce, envelope.ciphertext.as_ref())
            .map_err(|_| anyhow!("failed to decrypt relay secret"))?;
        String::from_utf8(plaintext).context("relay secret plaintext is not valid utf-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> String {
        STANDARD.encode([7_u8; KEY_BYTES])
    }

    #[test]
    fn relay_secret_round_trip() {
        let manager = RelaySecretManager::from_base64(&test_key()).expect("manager");
        let encrypted = manager.encrypt("secret").expect("encrypt");
        let decrypted = manager.decrypt(&encrypted).expect("decrypt");
        assert_eq!(decrypted, "secret");
    }

    #[test]
    fn relay_secret_rejects_wrong_key() {
        let manager = RelaySecretManager::from_base64(&test_key()).expect("manager");
        let encrypted = manager.encrypt("secret").expect("encrypt");
        let wrong_key = STANDARD.encode([9_u8; KEY_BYTES]);
        let wrong_manager = RelaySecretManager::from_base64(&wrong_key).expect("wrong manager");
        assert!(wrong_manager.decrypt(&encrypted).is_err());
    }
}
