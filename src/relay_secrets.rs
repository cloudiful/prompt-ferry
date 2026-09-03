use crate::runtime_env;
use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const KEY_VERSION_V1: i16 = 1;

/// File name of the auto-generated worker configuration encryption key stored
/// under the resolved app data root.
pub const WORKER_CONFIG_KEY_FILE: &str = "worker-config.key";

/// File name of the one-time bootstrap admin password file stored under the
/// resolved app data root.
pub const BOOTSTRAP_ADMIN_PASSWORD_FILE: &str = "bootstrap-admin.txt";

/// Generate a strong random bootstrap admin password for first startup. The
/// generated value is written once to a protected local file by the caller
/// and never logged.
pub(crate) fn generate_bootstrap_password() -> String {
    use rand::distr::{Alphanumeric, SampleString};
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

/// Resolve a generated-secret file location. Callers may pass an explicit
/// directory override (used by tests so they never touch the real platform
/// data root); production resolves `<data-root>/prompt-ferry/<file_name>`.
pub fn resolve_data_file(file_name: &str, dir_override: Option<&Path>) -> anyhow::Result<PathBuf> {
    match dir_override {
        Some(dir) => Ok(dir.join(file_name)),
        None => runtime_env::prompt_ferry_data_file(file_name),
    }
}

/// Resolve the effective worker configuration encryption key for a worker
/// start, logging which source provided it. `dir_override` restricts where a
/// generated key file is created; see [`resolve_data_file`].
pub fn load_or_create_worker_config_key_for(
    configured: &str,
    dir_override: Option<&Path>,
) -> anyhow::Result<RelaySecretManager> {
    let key_path = resolve_data_file(WORKER_CONFIG_KEY_FILE, dir_override)?;
    let (manager, source) = load_or_create_worker_config_key_at(configured, &key_path)?;
    match source {
        WorkerConfigKeySource::Configured => tracing::info!(
            "using configured worker configuration encryption key (WORKER_CONFIG_ENCRYPTION_KEY)"
        ),
        WorkerConfigKeySource::KeyFile(path) => tracing::info!(
            key_file = %path.display(),
            "loaded worker configuration encryption key from key file"
        ),
        WorkerConfigKeySource::Generated(path) => tracing::warn!(
            key_file = %path.display(),
            "generated a new random worker configuration encryption key and saved it with \
             restricted permissions; back up this file together with the database or encrypted \
             secrets become unreadable"
        ),
    }
    Ok(manager)
}

/// Where the effective worker configuration encryption key came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerConfigKeySource {
    /// Explicitly provided through configuration or environment.
    Configured,
    /// Loaded from an existing persisted key file.
    KeyFile(PathBuf),
    /// Newly generated and persisted for the first time.
    Generated(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedSecretEnvelope {
    #[serde(with = "base64_bytes")]
    pub ciphertext: Vec<u8>,
    #[serde(with = "base64_bytes")]
    pub nonce: Vec<u8>,
    pub key_version: i16,
}

mod base64_bytes {
    use super::STANDARD;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        STANDARD
            .decode(value.as_bytes())
            .map_err(serde::de::Error::custom)
    }
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
                "worker configuration encryption key is required for encrypted secret storage"
            ));
        }
        let bytes = STANDARD
            .decode(value.as_bytes())
            .context("worker configuration encryption key must be valid base64")?;
        let key = bytes.try_into().map_err(|bytes: Vec<u8>| {
            anyhow!(
                "worker configuration encryption key must decode to {KEY_BYTES} bytes, got {}",
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
            .map_err(|_| anyhow!("failed to encrypt worker configuration secret"))?;
        Ok(EncryptedSecretEnvelope {
            ciphertext,
            nonce: nonce.to_vec(),
            key_version: KEY_VERSION_V1,
        })
    }

    pub fn decrypt(&self, envelope: &EncryptedSecretEnvelope) -> anyhow::Result<String> {
        if envelope.key_version != KEY_VERSION_V1 {
            return Err(anyhow!(
                "unsupported worker configuration secret key version {}",
                envelope.key_version
            ));
        }
        let nonce: &[u8; NONCE_BYTES] = envelope.nonce.as_slice().try_into().map_err(|_| {
            anyhow!("worker configuration secret nonce must be {NONCE_BYTES} bytes")
        })?;
        let nonce = Nonce::from(*nonce);
        let cipher = ChaCha20Poly1305::new((&self.key).into());
        let plaintext = cipher
            .decrypt(&nonce, envelope.ciphertext.as_ref())
            .map_err(|_| anyhow!("failed to decrypt worker configuration secret"))?;
        String::from_utf8(plaintext).context("secret plaintext is not valid utf-8")
    }
}

/// Same resolution with an explicit key-file location, used by callers and
/// tests that own their data directory.
pub fn load_or_create_worker_config_key_at(
    configured: &str,
    key_path: &Path,
) -> anyhow::Result<(RelaySecretManager, WorkerConfigKeySource)> {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        return Ok((
            RelaySecretManager::from_base64(trimmed)?,
            WorkerConfigKeySource::Configured,
        ));
    }
    if let Some(manager) = read_key_file(key_path)? {
        return Ok((
            manager,
            WorkerConfigKeySource::KeyFile(key_path.to_path_buf()),
        ));
    }

    let mut bytes = [0_u8; KEY_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    let encoded = STANDARD.encode(bytes);
    runtime_env::create_private_file_exclusive(key_path, &format!("{encoded}\n"))?;
    // Re-read so concurrent starters converge on whichever process won the
    // exclusive create; both outcomes yield the same persisted key.
    match read_key_file(key_path)? {
        Some(manager) => Ok((
            manager,
            WorkerConfigKeySource::Generated(key_path.to_path_buf()),
        )),
        None => Err(anyhow!(
            "failed to persist worker configuration encryption key to {}",
            key_path.display()
        )),
    }
}

fn read_key_file(path: &Path) -> anyhow::Result<Option<RelaySecretManager>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow!(error).context(format!(
                "failed to read worker configuration encryption key file {}",
                path.display()
            )));
        }
    };
    let value = content.trim();
    if value.is_empty() {
        return Err(anyhow!(
            "worker configuration encryption key file {} is empty; restore the original key or \
             delete the file to generate a new one (existing encrypted secrets will no longer \
             decrypt)",
            path.display()
        ));
    }
    RelaySecretManager::from_base64(value)
        .map(Some)
        .map_err(|error| {
            error.context(format!(
                "invalid worker configuration encryption key file {}; restore the original key or \
                 delete the file to generate a new one (existing encrypted secrets will no longer \
                 decrypt)",
                path.display()
            ))
        })
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

    #[test]
    fn rejects_malformed_and_wrong_length_keys_with_actionable_errors() {
        let error = RelaySecretManager::from_base64("not-base64!!").unwrap_err();
        assert!(error.to_string().contains("valid base64"));

        let error = RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 16])).unwrap_err();
        assert!(error.to_string().contains("32 bytes"));

        let error = RelaySecretManager::from_base64("").unwrap_err();
        assert!(error.to_string().contains("encryption key is required"));
    }

    fn temp_key_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("prompt-ferry-key-{name}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn generates_persists_and_reuses_key_file() {
        let path = temp_key_path("generate");
        let (first, source) = load_or_create_worker_config_key_at("", &path).expect("generate key");
        assert_eq!(source, WorkerConfigKeySource::Generated(path.clone()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Restart simulation: the same key loads again from the file.
        let (second, source) = load_or_create_worker_config_key_at("", &path).expect("reload key");
        assert_eq!(source, WorkerConfigKeySource::KeyFile(path.clone()));
        let envelope = first.encrypt("round-trip").unwrap();
        assert_eq!(second.decrypt(&envelope).unwrap(), "round-trip");

        // An explicitly configured key wins and does not rewrite the file.
        let before = std::fs::read_to_string(&path).unwrap();
        let (configured, source) =
            load_or_create_worker_config_key_at(&test_key(), &path).expect("configured key");
        assert_eq!(source, WorkerConfigKeySource::Configured);
        assert!(configured.decrypt(&envelope).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn invalid_key_files_fail_without_overwriting() {
        let path = temp_key_path("malformed");
        std::fs::write(&path, "not-a-valid-key\n").unwrap();

        let error = load_or_create_worker_config_key_at("", &path).unwrap_err();
        assert!(error.to_string().contains(&path.display().to_string()));

        let path_short = temp_key_path("short");
        std::fs::write(&path_short, STANDARD.encode([1_u8; 8])).unwrap();
        let error = load_or_create_worker_config_key_at("", &path_short).unwrap_err();
        assert!(error.to_string().contains("delete the file"));

        let path_empty = temp_key_path("empty");
        std::fs::write(&path_empty, "   \n").unwrap();
        let error = load_or_create_worker_config_key_at("", &path_empty).unwrap_err();
        assert!(error.to_string().contains("is empty"));
    }
}
