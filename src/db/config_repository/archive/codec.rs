//! Binary codec for the versioned configuration archive.
//!
//! Layout (all integers big-endian):
//!
//! ```text
//! magic(8) | version(u16) | backend(u8) | reserved(u8) | salt(16) | nonce(12)
//!   | ciphertext_len(u32) | ciphertext
//! ```
//!
//! The clear-text header is authenticated as AEAD associated data, so a
//! tampered version, backend tag, salt, nonce, or length is rejected before
//! the payload is trusted. The key comes from Argon2id over the caller's
//! passphrase and the random per-archive salt; the sealed plaintext is
//! zstd-compressed JSON. Neither the passphrase nor any payload byte is ever
//! logged or echoed by this module.

use argon2::Argon2;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use rand::Rng;
use std::io::{Read, Write};

use super::error::ArchiveError;
use crate::db::config_repository::snapshot::{ConfigBackendKind, ConfigSnapshot};

pub const ARCHIVE_MAGIC: [u8; 8] = *b"PFCEXP01";
pub const ARCHIVE_FORMAT_VERSION: u16 = 1;
pub const MIN_PASSPHRASE_LEN: usize = 12;
pub const MAX_PASSPHRASE_LEN: usize = 1024;
/// Upper bound on a whole encoded archive accepted by the decoder.
pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
/// Upper bound on the decompressed snapshot JSON. Guards against a
/// decompression bomb hidden inside a small ciphertext.
pub const MAX_PLAINTEXT_BYTES: usize = 192 * 1024 * 1024;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// ChaCha20-Poly1305 authentication tag length appended to the ciphertext.
const AEAD_TAG_LEN: usize = 16;
const HEADER_LEN: usize = 8 + 2 + 1 + 1 + SALT_LEN + NONCE_LEN + 4;
const ZSTD_LEVEL: i32 = 10;

/// A sealed archive plus the fingerprint of the payload it carries.
pub struct EncodedArchive {
    pub bytes: Vec<u8>,
    pub payload_fingerprint: String,
}

pub fn validate_passphrase(passphrase: &str) -> Result<(), ArchiveError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
        return Err(ArchiveError::PassphraseTooShort {
            min: MIN_PASSPHRASE_LEN,
        });
    }
    if passphrase.chars().count() > MAX_PASSPHRASE_LEN {
        return Err(ArchiveError::PassphraseTooLong {
            max: MAX_PASSPHRASE_LEN,
        });
    }
    Ok(())
}

/// Seal a snapshot into an encrypted archive. `snapshot` is resealed in place
/// so the manifest describes exactly the bytes that get compressed.
pub fn encode_archive(
    passphrase: &str,
    backend: ConfigBackendKind,
    mut snapshot: ConfigSnapshot,
) -> Result<EncodedArchive, ArchiveError> {
    validate_passphrase(passphrase)?;
    if snapshot.manifest.backend_kind != backend {
        return Err(ArchiveError::BackendMismatch {
            expected: backend,
            found: snapshot.manifest.backend_kind,
        });
    }
    snapshot.seal().map_err(|_| ArchiveError::Serialization)?;
    let plaintext = serde_json::to_vec(&snapshot).map_err(|_| ArchiveError::Serialization)?;
    if plaintext.len() > MAX_PLAINTEXT_BYTES {
        return Err(ArchiveError::TooLarge {
            limit: MAX_PLAINTEXT_BYTES,
        });
    }
    let compressed = compress(&plaintext)?;

    let mut salt = [0_u8; SALT_LEN];
    let mut nonce = [0_u8; NONCE_LEN];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce);
    let key = derive_key(passphrase, &salt)?;

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(&ARCHIVE_MAGIC);
    header.extend_from_slice(&ARCHIVE_FORMAT_VERSION.to_be_bytes());
    header.push(backend_tag(backend));
    header.push(0);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    header.extend_from_slice(&((compressed.len() + AEAD_TAG_LEN) as u32).to_be_bytes());

    let ciphertext = ChaCha20Poly1305::new((&key).into())
        .encrypt(
            &Nonce::from(nonce),
            chacha20poly1305::aead::Payload {
                msg: &compressed,
                aad: &header,
            },
        )
        .map_err(|_| ArchiveError::Serialization)?;

    let mut bytes = header;
    bytes.extend_from_slice(&ciphertext);
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge {
            limit: MAX_ARCHIVE_BYTES,
        });
    }
    Ok(EncodedArchive {
        bytes,
        payload_fingerprint: snapshot.manifest.payload_fingerprint.clone(),
    })
}

/// Open an archive without checking it against a target backend. Used by the
/// dry-run preview path; the caller still has to compare backends before an
/// import.
pub fn decode_archive(passphrase: &str, bytes: &[u8]) -> Result<ConfigSnapshot, ArchiveError> {
    decode_archive_for_backend(passphrase, bytes, None)
}

/// Open an archive, optionally requiring it to have been written by
/// `expected_backend`.
pub fn decode_archive_for_backend(
    passphrase: &str,
    bytes: &[u8],
    expected_backend: Option<ConfigBackendKind>,
) -> Result<ConfigSnapshot, ArchiveError> {
    validate_passphrase(passphrase)?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::TooLarge {
            limit: MAX_ARCHIVE_BYTES,
        });
    }
    if bytes.len() < HEADER_LEN + AEAD_TAG_LEN {
        return Err(ArchiveError::InvalidHeader);
    }
    if bytes[..ARCHIVE_MAGIC.len()] != ARCHIVE_MAGIC {
        return Err(ArchiveError::InvalidHeader);
    }
    let mut cursor = ARCHIVE_MAGIC.len();
    let version = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]);
    cursor += 2;
    if version != ARCHIVE_FORMAT_VERSION {
        return Err(ArchiveError::UnsupportedVersion {
            found: version,
            supported: ARCHIVE_FORMAT_VERSION,
        });
    }
    let backend = backend_from_tag(bytes[cursor]).ok_or(ArchiveError::InvalidHeader)?;
    cursor += 1;
    if bytes[cursor] != 0 {
        return Err(ArchiveError::InvalidHeader);
    }
    cursor += 1;
    let salt: [u8; SALT_LEN] = bytes[cursor..cursor + SALT_LEN]
        .try_into()
        .map_err(|_| ArchiveError::InvalidHeader)?;
    cursor += SALT_LEN;
    let nonce: [u8; NONCE_LEN] = bytes[cursor..cursor + NONCE_LEN]
        .try_into()
        .map_err(|_| ArchiveError::InvalidHeader)?;
    cursor += NONCE_LEN;
    let ciphertext_len = u32::from_be_bytes(
        bytes[cursor..cursor + 4]
            .try_into()
            .map_err(|_| ArchiveError::InvalidHeader)?,
    ) as usize;
    if bytes.len() != HEADER_LEN + ciphertext_len {
        return Err(ArchiveError::InvalidHeader);
    }
    if let Some(expected) = expected_backend
        && expected != backend
    {
        return Err(ArchiveError::BackendMismatch {
            expected,
            found: backend,
        });
    }

    let header = &bytes[..HEADER_LEN];
    let ciphertext = &bytes[HEADER_LEN..];
    let key = derive_key(passphrase, &salt)?;
    let compressed = ChaCha20Poly1305::new((&key).into())
        .decrypt(
            &Nonce::from(nonce),
            chacha20poly1305::aead::Payload {
                msg: ciphertext,
                aad: header,
            },
        )
        .map_err(|_| ArchiveError::DecryptionFailed)?;
    let plaintext = decompress(&compressed)?;
    let snapshot: ConfigSnapshot = serde_json::from_slice(&plaintext)
        .map_err(|_| ArchiveError::InvalidPayload("unreadable snapshot"))?;
    if snapshot.manifest.backend_kind != backend {
        return Err(ArchiveError::InvalidPayload(
            "backend tag does not match the payload",
        ));
    }
    snapshot
        .validate()
        .map_err(|_| ArchiveError::InvalidPayload("snapshot failed validation"))?;
    Ok(snapshot)
}

fn backend_tag(kind: ConfigBackendKind) -> u8 {
    match kind {
        ConfigBackendKind::Postgres => 1,
        ConfigBackendKind::Sqlite => 2,
    }
}

fn backend_from_tag(tag: u8) -> Option<ConfigBackendKind> {
    match tag {
        1 => Some(ConfigBackendKind::Postgres),
        2 => Some(ConfigBackendKind::Sqlite),
        _ => None,
    }
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], ArchiveError> {
    let mut key = [0_u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|_| ArchiveError::KeyDerivation)?;
    Ok(key)
}

fn compress(plaintext: &[u8]) -> Result<Vec<u8>, ArchiveError> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), ZSTD_LEVEL)
        .map_err(|_| ArchiveError::Compression)?;
    encoder
        .write_all(plaintext)
        .map_err(|_| ArchiveError::Compression)?;
    encoder.finish().map_err(|_| ArchiveError::Compression)
}

fn decompress(compressed: &[u8]) -> Result<Vec<u8>, ArchiveError> {
    decompress_with_limit(compressed, MAX_PLAINTEXT_BYTES)
}

/// Bounded decompression: a payload that would inflate past `limit` is
/// rejected as too large instead of being buffered.
fn decompress_with_limit(compressed: &[u8], limit: usize) -> Result<Vec<u8>, ArchiveError> {
    let decoder =
        zstd::stream::read::Decoder::new(compressed).map_err(|_| ArchiveError::Compression)?;
    let mut plaintext = Vec::new();
    decoder
        .take((limit + 1) as u64)
        .read_to_end(&mut plaintext)
        .map_err(|_| ArchiveError::Compression)?;
    if plaintext.len() > limit {
        return Err(ArchiveError::TooLarge { limit });
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_bounds_are_enforced() {
        assert!(matches!(
            validate_passphrase("short"),
            Err(ArchiveError::PassphraseTooShort { .. })
        ));
        assert!(validate_passphrase("long-enough-passphrase").is_ok());
        let too_long = "x".repeat(MAX_PASSPHRASE_LEN + 1);
        assert!(matches!(
            validate_passphrase(&too_long),
            Err(ArchiveError::PassphraseTooLong { .. })
        ));
    }

    #[test]
    fn bounded_decompression_rejects_inflation_past_the_limit() {
        let payload = vec![b'a'; 4096];
        let compressed = compress(&payload).expect("compress");
        assert_eq!(
            decompress_with_limit(&compressed, 4096).expect("within limit"),
            payload
        );
        assert!(matches!(
            decompress_with_limit(&compressed, 1024),
            Err(ArchiveError::TooLarge { limit: 1024 })
        ));
    }
}
