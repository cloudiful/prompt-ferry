//! Archive codec tests: round trip, tamper resistance, and input bounds.

use super::archive_test_fixtures::{
    GRANT_OWNER_USER_ID, GRANT_SERVER_ID, LEGACY_FINGERPRINT, LEGACY_MONEY_FINGERPRINT,
    LEGACY_MONEY_PAYLOAD_JSON, LEGACY_PAYLOAD_JSON, PASSPHRASE, sample_grant, sample_snapshot,
    snapshot_with_grant,
};

use crate::db::config_repository::archive::{
    ARCHIVE_FORMAT_VERSION, ArchiveError, MAX_ARCHIVE_BYTES, MIN_PASSPHRASE_LEN, decode_archive,
    decode_archive_for_backend, decode_payload_for_tests, encode_archive, validate_passphrase,
};
use crate::db::config_repository::snapshot::{ConfigBackendKind, SNAPSHOT_FORMAT_VERSION};

#[test]
fn archive_round_trip_preserves_the_manifest_and_domains() {
    let snapshot = sample_snapshot(ConfigBackendKind::Postgres);
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    assert!(encoded.bytes.starts_with(b"PFCEXP01"));
    assert_eq!(encoded.payload_fingerprint.len(), 64);

    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    assert_eq!(decoded.manifest.backend_kind, ConfigBackendKind::Postgres);
    assert_eq!(decoded.manifest.format_version, SNAPSHOT_FORMAT_VERSION);
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

#[test]
fn a_pinned_legacy_payload_is_verified_and_normalized_to_the_current_format() {
    let snapshot =
        decode_payload_for_tests(LEGACY_PAYLOAD_JSON.as_bytes(), ConfigBackendKind::Postgres)
            .expect("legacy payload decodes");
    assert_eq!(snapshot.manifest.format_version, SNAPSHOT_FORMAT_VERSION);
    let provenance = snapshot.manifest.provenance.as_ref().expect("provenance");
    assert_eq!(provenance.source_format_version, 2);
    assert_eq!(provenance.source_payload_fingerprint, LEGACY_FINGERPRINT);
    assert!(
        snapshot.domains.mcp_oauth.is_empty(),
        "a legacy payload carries no grant"
    );
    assert_eq!(
        snapshot
            .manifest
            .domains
            .iter()
            .map(|domain| domain.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "users",
            "client_keys",
            "endpoints",
            "model_routes",
            "mcp_servers",
            "mcp_credentials",
            "settings",
            "user_redaction_configs",
            "mcp_oauth",
            "user_endpoint_settings",
        ],
        "the normalized manifest declares the ten current domains"
    );
    let serialized = serde_json::to_string(&snapshot.domains).expect("domains json");
    assert!(!serialized.contains("raw_object_store"));
    assert!(!serialized.contains("default_cost"));
    snapshot
        .validate()
        .expect("the normalized payload validates against its new fingerprint");
}

#[test]
fn a_normalized_legacy_payload_re_encodes_as_the_current_format() {
    let normalized = decode_payload_for_tests(
        LEGACY_MONEY_PAYLOAD_JSON.as_bytes(),
        ConfigBackendKind::Postgres,
    )
    .expect("money payload decodes");
    let encoded = encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, normalized)
        .expect("encode normalized archive");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    assert_eq!(decoded.manifest.format_version, SNAPSHOT_FORMAT_VERSION);
    assert_eq!(
        decoded
            .manifest
            .provenance
            .as_ref()
            .map(|provenance| provenance.source_format_version),
        Some(2),
        "the normalized archive keeps its provenance through a new container"
    );
    assert_eq!(
        decoded.manifest.payload_fingerprint,
        encoded.payload_fingerprint
    );
}

#[test]
fn a_grant_keeps_every_portable_field_through_the_archive() {
    let snapshot = snapshot_with_grant(ConfigBackendKind::Postgres);
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");

    assert_eq!(decoded.domains.mcp_oauth.len(), 1);
    let grant = &decoded.domains.mcp_oauth[0];
    let expected = sample_grant();
    assert_eq!(grant.server_id.to_string(), GRANT_SERVER_ID);
    assert_eq!(grant.owner_user_id, GRANT_OWNER_USER_ID);
    assert_eq!(grant.config_fingerprint, expected.config_fingerprint);
    assert_eq!(grant.registration.snapshot, expected.registration.snapshot);
    assert_eq!(grant.credentials.engine, expected.credentials.engine);
    assert_eq!(grant.credentials.scopes, expected.credentials.scopes);
    assert_eq!(
        grant.credentials.expires_at,
        expected.credentials.expires_at
    );
    assert_eq!(grant.credentials.issuer, expected.credentials.issuer);

    let summary = decoded
        .manifest
        .domains
        .iter()
        .find(|domain| domain.name == "mcp_oauth")
        .expect("the manifest declares the personal OAuth domain");
    assert_eq!(summary.records, 1);
}

#[test]
fn a_grant_carries_plaintext_and_never_source_ciphertext() {
    let snapshot = snapshot_with_grant(ConfigBackendKind::Postgres);
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    let plaintext = serde_json::to_string(&sample_grant()).expect("grant json");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    let grant_json = serde_json::to_string(&decoded.domains.mcp_oauth[0]).expect("grant json");
    assert!(
        grant_json.contains("archived-access-token"),
        "the grant travels as plaintext inside the encrypted archive"
    );
    for source_only in ["ciphertext", "nonce", "key_version"] {
        assert!(
            !grant_json.contains(source_only),
            "the grant never carries the source {source_only}"
        );
    }
    assert!(plaintext.contains("archived-client"));
}

#[test]
fn a_different_master_cannot_read_the_source_sealed_grant() {
    use crate::relay_secrets::RelaySecretManager;
    let source = RelaySecretManager::from_key_bytes([7_u8; 32]);
    let destination = RelaySecretManager::from_key_bytes([9_u8; 32]);
    let envelope = source.encrypt("archived-access-token").expect("encrypt");
    assert!(
        destination.decrypt(&envelope).is_err(),
        "the destination master cannot read the source ciphertext"
    );
    assert!(source.decrypt(&envelope).is_ok());
}

#[test]
fn a_payload_with_retired_credential_money_values_is_normalized_with_a_count_only_note() {
    let snapshot = decode_payload_for_tests(
        LEGACY_MONEY_PAYLOAD_JSON.as_bytes(),
        ConfigBackendKind::Postgres,
    )
    .expect("money payload decodes");
    let provenance = snapshot.manifest.provenance.as_ref().expect("provenance");
    assert_eq!(
        provenance.source_payload_fingerprint, LEGACY_MONEY_FINGERPRINT,
        "the original fingerprint still matches the frozen bytes"
    );
    let note = provenance
        .discarded
        .iter()
        .find(|note| note.name == "mcp_credentials.retired_fields")
        .expect("the retired fields are counted");
    assert_eq!(note.records, 1);
    let credential = &snapshot.domains.mcp_credentials[0];
    assert_eq!(credential.secret, "fc-archived-secret");
    assert_eq!(credential.provider_kind.as_deref(), Some("firecrawl"));
    let serialized = serde_json::to_string(credential).expect("credential json");
    for retired in [
        "default_cost",
        "strict_mode",
        "billing_period_start",
        "billing_period_end",
    ] {
        assert!(
            !serialized.contains(retired),
            "the current credential shape never carries {retired}"
        );
    }
}

#[test]
fn a_payload_version_or_backend_mismatch_is_rejected() {
    assert!(matches!(
        decode_payload_for_tests(LEGACY_PAYLOAD_JSON.as_bytes(), ConfigBackendKind::Sqlite),
        Err(ArchiveError::InvalidPayload(
            "backend tag does not match the payload"
        ))
    ));
    let unknown = LEGACY_PAYLOAD_JSON.replace("\"format_version\":2", "\"format_version\":9");
    assert!(matches!(
        decode_payload_for_tests(unknown.as_bytes(), ConfigBackendKind::Postgres),
        Err(ArchiveError::UnsupportedVersion {
            found: 9,
            supported: SNAPSHOT_FORMAT_VERSION
        })
    ));
}

#[test]
fn a_current_export_serializes_only_the_ten_portable_domains() {
    let snapshot = snapshot_with_grant(ConfigBackendKind::Postgres);
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    assert!(
        decoded.manifest.provenance.is_none(),
        "a native export has no provenance"
    );
    let names: Vec<&str> = decoded
        .manifest
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    assert_eq!(names.len(), 10);
    assert!(!names.contains(&"raw_object_store"));

    let plaintext = serde_json::to_value(&decoded.domains).expect("domains json");
    for retired in [
        "raw_object_store",
        "default_cost",
        "strict_mode",
        "billing_period_start",
        "billing_period_end",
    ] {
        assert!(
            !plaintext.to_string().contains(retired),
            "a current payload never carries {retired}"
        );
    }
}

#[test]
fn a_frozen_nine_domain_v3_payload_still_decodes_with_an_empty_preference_domain() {
    let snapshot = decode_payload_for_tests(
        crate::db::config_repository::archive_test_fixtures::V3_NINE_PAYLOAD_JSON.as_bytes(),
        ConfigBackendKind::Postgres,
    )
    .expect("the old native V3 payload decodes");
    assert_eq!(
        snapshot.manifest.payload_fingerprint,
        crate::db::config_repository::archive_test_fixtures::V3_NINE_FINGERPRINT
    );
    assert_eq!(snapshot.manifest.domains.len(), 9);
    assert!(
        !snapshot
            .manifest
            .domains
            .iter()
            .any(|domain| domain.name == "user_endpoint_settings")
    );
    assert!(snapshot.domains.user_endpoint_settings.is_empty());
    assert_eq!(
        serde_json::to_string(&snapshot.domains).expect("domains json"),
        crate::db::config_repository::archive_test_fixtures::V3_NINE_DOMAINS_JSON,
        "the old payload bytes and fingerprint are unchanged"
    );
}

#[test]
fn explicit_preferences_round_trip_with_null_and_absent_states_distinct() {
    let mut snapshot = snapshot_with_grant(ConfigBackendKind::Postgres);
    let endpoint_id = uuid::Uuid::parse_str(GRANT_SERVER_ID).expect("endpoint id");
    snapshot.domains.user_endpoint_settings = vec![
        crate::db::config_repository::snapshot::UserEndpointSettingSnapshot {
            user_id: GRANT_OWNER_USER_ID,
            endpoint_id: Some(endpoint_id),
        },
        crate::db::config_repository::snapshot::UserEndpointSettingSnapshot {
            user_id: 2,
            endpoint_id: None,
        },
    ];
    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Postgres, snapshot).expect("encode archive");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode archive");
    assert_eq!(
        decoded.domains.user_endpoint_settings,
        vec![
            crate::db::config_repository::snapshot::UserEndpointSettingSnapshot {
                user_id: GRANT_OWNER_USER_ID,
                endpoint_id: Some(endpoint_id),
            },
            crate::db::config_repository::snapshot::UserEndpointSettingSnapshot {
                user_id: 2,
                endpoint_id: None,
            },
        ]
    );
    assert!(
        decoded
            .manifest
            .domains
            .iter()
            .any(|domain| domain.name == "user_endpoint_settings" && domain.records == 2)
    );
    assert_eq!(
        decoded.manifest.payload_fingerprint,
        encoded.payload_fingerprint
    );
}
