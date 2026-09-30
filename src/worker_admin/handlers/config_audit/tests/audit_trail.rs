//! What a real export/import attempt records, and what it must never record.

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tower::ServiceExt;

use crate::db::config_repository::{
    ConfigAuditAction, ConfigBackendKind, SNAPSHOT_FORMAT_VERSION, audit::MAX_AUDIT_ERROR_CHARS,
};
use crate::worker_admin;

#[tokio::test]
async fn successful_export_records_non_secret_metadata() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "audit-admin", true).await;
    seed_configuration(&state).await;
    let app = worker_admin::router(state.clone());

    let exported = app
        .clone()
        .oneshot(export_request(Some("audit-admin"), PASSPHRASE))
        .await
        .expect("export request");
    assert_eq!(exported.status(), StatusCode::OK);
    let fingerprint_header = exported
        .headers()
        .get("x-config-export-fingerprint")
        .and_then(|value| value.to_str().ok())
        .expect("fingerprint header")
        .to_string();
    let archive_len = axum::body::to_bytes(exported.into_body(), usize::MAX)
        .await
        .expect("archive body")
        .len() as i64;

    let page = audit_page(&state).await;
    assert_eq!(page.total, 1, "one export attempt, one audit row");
    let entry = &page.entries[0];
    assert_eq!(entry.action, ConfigAuditAction::Export);
    assert!(entry.success);
    assert_eq!(entry.backend_kind, ConfigBackendKind::Sqlite);
    assert_eq!(entry.format_version, Some(SNAPSHOT_FORMAT_VERSION));
    assert_eq!(entry.archive_bytes, Some(archive_len));
    assert_eq!(
        entry.payload_fingerprint.as_deref(),
        Some(fingerprint_header.as_str())
    );
    assert_eq!(entry.actor_user_id, Some(1));
    assert_eq!(entry.actor_login_name.as_deref(), Some("admin"));
    assert!(entry.error_code.is_none());
    assert!(entry.error_message.is_none());
    let domains: Vec<&str> = entry
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    assert!(domains.contains(&"users"), "domains: {domains:?}");
    assert!(domains.contains(&"endpoints"), "domains: {domains:?}");

    close_state(store, path).await;
}

#[tokio::test]
async fn audit_trail_records_failures_and_never_a_secret() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "audit-admin", true).await;
    seed_configuration(&state).await;
    let app = worker_admin::router(state.clone());

    // A rejected export produces no archive but is still an attempt.
    let rejected = app
        .clone()
        .oneshot(export_request(Some("audit-admin"), "short"))
        .await
        .expect("rejected export");
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    let rejected_audit_id = audit_page(&state).await.entries[0].audit_id;

    let archive = export_archive(&state).await;
    let archive_base64 = STANDARD.encode(&archive);

    // The read-only preview must not write anything.
    let before_preview = audit_page(&state).await.total;
    let preview = app
        .clone()
        .oneshot(import_request(
            "/api/v1/admin/config-import/preview",
            Some("audit-admin"),
            PASSPHRASE,
            &archive,
        ))
        .await
        .expect("preview request");
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(
        audit_page(&state).await.total,
        before_preview,
        "a preview is read-only and records nothing"
    );

    // A wrong passphrase is rejected after the archive was decoded.
    let wrong_passphrase = "audit-wrong-passphrase";
    let failed = app
        .clone()
        .oneshot(import_request(
            "/api/v1/admin/config-import",
            Some("audit-admin"),
            wrong_passphrase,
            &archive,
        ))
        .await
        .expect("failed import");
    assert_eq!(failed.status(), StatusCode::BAD_REQUEST);

    // The committed import lands and is audited.
    let applied = app
        .clone()
        .oneshot(import_request(
            "/api/v1/admin/config-import",
            Some("audit-admin"),
            PASSPHRASE,
            &archive,
        ))
        .await
        .expect("committed import");
    assert_eq!(applied.status(), StatusCode::OK);

    let page = audit_page(&state).await;
    assert_eq!(page.total, 3);
    // A restore replaces configuration domains only; the trail written before
    // the import is still there afterwards.
    assert!(
        page.entries
            .iter()
            .any(|entry| entry.audit_id == rejected_audit_id),
        "the import must not clear the existing audit trail"
    );

    let rejected = page
        .entries
        .iter()
        .find(|entry| entry.action == ConfigAuditAction::Export)
        .expect("rejected export row");
    assert!(!rejected.success);
    assert_eq!(
        rejected.error_code.as_deref(),
        Some("archive_passphrase_too_short")
    );
    assert!(rejected.payload_fingerprint.is_none());
    assert!(rejected.archive_bytes.is_none());
    assert!(rejected.domains.is_empty());
    let summary = rejected.error_message.as_deref().expect("error summary");
    assert!(
        summary.chars().count() <= MAX_AUDIT_ERROR_CHARS,
        "summary is bounded: {summary:?}"
    );

    let failed = page
        .entries
        .iter()
        .find(|entry| !entry.success && entry.action == ConfigAuditAction::Import)
        .expect("failed import row");
    assert_eq!(failed.error_code.as_deref(), Some("archive_decrypt_failed"));
    assert_eq!(failed.archive_bytes, Some(archive.len() as i64));
    assert!(failed.payload_fingerprint.is_none());

    let applied = page
        .entries
        .iter()
        .find(|entry| entry.success && entry.action == ConfigAuditAction::Import)
        .expect("committed import row");
    assert_eq!(applied.backend_kind, ConfigBackendKind::Sqlite);
    assert_eq!(applied.format_version, Some(SNAPSHOT_FORMAT_VERSION));
    assert_eq!(applied.archive_bytes, Some(archive.len() as i64));
    assert!(
        applied
            .payload_fingerprint
            .as_deref()
            .is_some_and(|fingerprint| fingerprint.len() == 64),
        "import rows keep the archive fingerprint"
    );

    // The stored trail must not carry the passphrase, a seeded secret, or any
    // part of the archive envelope itself.
    let serialized = serde_json::to_string(&page).expect("serialize audit page");
    for forbidden in [
        PASSPHRASE,
        wrong_passphrase,
        "endpoint-secret",
        "admin-key",
        "bearer-one",
        archive_base64.as_str(),
    ] {
        assert!(
            !serialized.contains(forbidden),
            "audit trail leaked {forbidden:?}"
        );
    }

    close_state(store, path).await;
}
