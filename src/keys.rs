use crate::naming::CLIENT_KEY_PREFIX;
use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};

const KEY_BYTES: usize = 32;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| anyhow::anyhow!("failed to hash password: {err}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn generate_client_key() -> (String, String, String) {
    let mut bytes = [0_u8; KEY_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    let secret = format!("{CLIENT_KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let prefix = secret.chars().take(12).collect::<String>();
    let hash = hash_client_key(&secret);
    (secret, prefix, hash)
}

pub fn hash_client_key(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_round_trips() {
        let hash = hash_password("password-123").unwrap();
        assert!(verify_password("password-123", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn client_key_has_prefix_and_hash() {
        let (secret, prefix, hash) = generate_client_key();
        assert!(secret.starts_with(CLIENT_KEY_PREFIX));
        assert!(secret.starts_with(&prefix));
        assert_eq!(hash_client_key(&secret), hash);
    }
}
