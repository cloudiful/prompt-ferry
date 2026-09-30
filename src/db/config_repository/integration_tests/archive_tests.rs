//! Archive codec tests: round trip, tamper resistance, and input bounds.

use super::snapshot_fixtures::{PASSPHRASE, sample_snapshot};

use crate::db::config_repository::archive::{
    ARCHIVE_FORMAT_VERSION, ArchiveError, MAX_ARCHIVE_BYTES, MIN_PASSPHRASE_LEN, decode_archive,
    decode_archive_for_backend, encode_archive, validate_passphrase,
};
use crate::db::config_repository::snapshot::ConfigBackendKind;

#[test]
fn archive_round_trip_preserves_the_manifest_and_domains() {
    let snapshot = sample_snapshot(ConfigBackendKind::Postgres);
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    assert!(encoded.bytes.starts_with(b"PFCEXP01"));
    assert_eq!(encoded.payload_fingerprint.len(), 64);

    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    assert_eq!(decoded.manifest.backend_kind, ConfigBackendKind::Postgres);
    assert_eq!(decoded.manifest.format_version, ARCHIVE_FORMAT_VERSION);
    assert_eq!(
        decoded.manifest.payload_fingerprint,
        encoded.payload_fingerprint
    );
    assert_eq!(decoded.domains.users.len(), 1);
    assert_eq!(decoded.domains.users[0].login_name, "admin");
    assert_eq!(decoded.domains.settings.len(), 1);
    decoded.validate().expect("decoded snapshot validates");
}

#[test]
fn archive_is_not_deterministic_and_never_contains_the_passphrase() {
    let first = encode_archive(
        PASSPHRASE,
        ConfigBackendKind::Postgres,
        sample_snapshot(ConfigBackendKind::Postgres),
    )
    .expect("first encode");
    let second = encode_archive(
        PASSPHRASE,
        ConfigBackendKind::Postgres,
        sample_snapshot(ConfigBackendKind::Postgres),
    )
    .expect("second encode");
    assert_ne!(first.bytes, second.bytes, "random salt/nonce per archive");
    assert_eq!(
        first.payload_fingerprint, second.payload_fingerprint,
        "payload fingerprint is stable"
    );
    let needle = PASSPHRASE.as_bytes();
    assert!(
        !first
            .bytes
            .windows(needle.len())
            .any(|window| window == needle),
        "the passphrase must never appear in the sealed bytes"
    );
}

#[test]
fn wrong_passphrase_and_tampered_bytes_are_rejected() {
    let encoded = encode_archive(
        PASSPHRASE,
        ConfigBackendKind::Sqlite,
        sample_snapshot(ConfigBackendKind::Sqlite),
    )
    .expect("encode archive");

    assert!(matches!(
        decode_archive("another passphrase entirely", &encoded.bytes),
        Err(ArchiveError::DecryptionFailed)
    ));

    let mut tampered_ciphertext = encoded.bytes.clone();
    let last = tampered_ciphertext.len() - 1;
    tampered_ciphertext[last] ^= 0x01;
    assert!(matches!(
        decode_archive(PASSPHRASE, &tampered_ciphertext),
        Err(ArchiveError::DecryptionFailed)
    ));

    // The salt and nonce live inside the authenticated header, so flipping
    // either fails the same way as a ciphertext bit.
    let mut tampered_salt = encoded.bytes.clone();
    tampered_salt[12] ^= 0x01;
    assert!(matches!(
        decode_archive(PASSPHRASE, &tampered_salt),
        Err(ArchiveError::DecryptionFailed)
    ));

    // The declared ciphertext length is also authenticated, and a mismatch
    // against the actual body is rejected before any key derivation.
    let mut tampered_length = encoded.bytes.clone();
    let length_offset = 8 + 2 + 1 + 1 + 16 + 12;
    tampered_length[length_offset] ^= 0x01;
    assert!(matches!(
        decode_archive(PASSPHRASE, &tampered_length),
        Err(ArchiveError::InvalidHeader)
    ));
}

#[test]
fn unsupported_version_backend_mismatch_and_truncation_are_rejected() {
    let encoded = encode_archive(
        PASSPHRASE,
        ConfigBackendKind::Postgres,
        sample_snapshot(ConfigBackendKind::Postgres),
    )
    .expect("encode archive");

    let mut newer_version = encoded.bytes.clone();
    newer_version[8..10].copy_from_slice(&(ARCHIVE_FORMAT_VERSION + 1).to_be_bytes());
    assert!(matches!(
        decode_archive(PASSPHRASE, &newer_version),
        Err(ArchiveError::UnsupportedVersion { .. })
    ));

    assert!(matches!(
        decode_archive_for_backend(PASSPHRASE, &encoded.bytes, Some(ConfigBackendKind::Sqlite)),
        Err(ArchiveError::BackendMismatch {
            expected: ConfigBackendKind::Sqlite,
            found: ConfigBackendKind::Postgres,
        })
    ));

    let mut unknown_backend = encoded.bytes.clone();
    unknown_backend[10] = 7;
    assert!(matches!(
        decode_archive(PASSPHRASE, &unknown_backend),
        Err(ArchiveError::InvalidHeader)
    ));

    assert!(matches!(
        decode_archive(PASSPHRASE, &encoded.bytes[..20]),
        Err(ArchiveError::InvalidHeader)
    ));
    assert!(matches!(
        decode_archive(PASSPHRASE, b"not-an-archive-at-all-not-an-archive"),
        Err(ArchiveError::InvalidHeader)
    ));
}

#[test]
fn oversized_inputs_are_rejected_before_decryption() {
    let oversized = vec![0_u8; MAX_ARCHIVE_BYTES + 1];
    assert!(matches!(
        decode_archive(PASSPHRASE, &oversized),
        Err(ArchiveError::TooLarge { .. })
    ));
    assert!(matches!(
        validate_passphrase(&"x".repeat(MIN_PASSPHRASE_LEN - 1)),
        Err(ArchiveError::PassphraseTooShort { .. })
    ));
}
