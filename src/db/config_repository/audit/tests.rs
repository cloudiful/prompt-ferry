use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;

use crate::db;
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::StandaloneConfigStore;
const MANAGER_KEY: [u8; 32] = [3_u8; 32];

async fn sqlite_repository() -> (
    ConfigRepository,
    Arc<StandaloneConfigStore>,
    std::path::PathBuf,
) {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-audit-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    db::UserStore::sqlite(store.pool().clone())
        .bootstrap_admin("admin", "admin-password")
        .await
        .expect("bootstrap admin");
    let manager =
        RelaySecretManager::from_base64(&STANDARD.encode(MANAGER_KEY)).expect("secret manager");
    (
        ConfigRepository::sqlite(store.clone(), manager),
        store,
        path,
    )
}

fn domains() -> Vec<ConfigAuditDomainCount> {
    vec![
        ConfigAuditDomainCount {
            name: "users".to_string(),
            records: 2,
            unrecoverable_secrets: 0,
        },
        ConfigAuditDomainCount {
            name: "client_keys".to_string(),
            records: 1,
            unrecoverable_secrets: 1,
        },
    ]
}

#[tokio::test]
async fn audit_rows_round_trip_through_the_standalone_backend() {
    let (repository, store, path) = sqlite_repository().await;
    let record = ConfigAuditRecord {
        actor_user_id: Some(1),
        action: ConfigAuditAction::Import,
        backend_kind: ConfigBackendKind::Sqlite,
        format_version: Some(SNAPSHOT_FORMAT_VERSION),
        archive_bytes: Some(4096),
        payload_fingerprint: Some("abc123".to_string()),
        success: true,
        error_code: None,
        error_message: None,
        domains: domains(),
    };
    let audit_id = repository
        .record_config_audit(&record)
        .await
        .expect("record audit");
    assert!(audit_id > 0);

    let page = repository
        .list_config_audit(0, 10)
        .await
        .expect("list audit");
    assert_eq!(page.total, 1);
    assert_eq!(page.first, 0);
    assert_eq!(page.rows, 10);
    let entry = &page.entries[0];
    assert_eq!(entry.audit_id, audit_id);
    assert_eq!(entry.actor_user_id, Some(1));
    assert_eq!(entry.actor_login_name.as_deref(), Some("admin"));
    assert_eq!(entry.action, ConfigAuditAction::Import);
    assert_eq!(entry.backend_kind, ConfigBackendKind::Sqlite);
    assert_eq!(entry.format_version, Some(SNAPSHOT_FORMAT_VERSION));
    assert_eq!(entry.archive_bytes, Some(4096));
    assert_eq!(entry.payload_fingerprint.as_deref(), Some("abc123"));
    assert!(entry.success);
    assert!(entry.error_code.is_none());
    assert_eq!(entry.domains, domains());

    store.pool().close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn failed_attempts_record_only_a_redacted_bounded_summary() {
    let (repository, store, path) = sqlite_repository().await;
    let secret = "sk-live-super-secret-value";
    let record = ConfigAuditRecord::failure(
        None,
        ConfigAuditAction::Import,
        ConfigBackendKind::Sqlite,
        Some(12),
        "archive_decrypt_failed",
        &format!("{} {secret}", "x".repeat(4096)),
    );
    let stored = record.error_message.clone().expect("error message");
    assert!(stored.chars().count() <= MAX_AUDIT_ERROR_CHARS);
    repository
        .record_config_audit(&record)
        .await
        .expect("record audit");

    let page = repository
        .list_config_audit(0, 10)
        .await
        .expect("list audit");
    let entry = &page.entries[0];
    assert!(!entry.success);
    assert_eq!(entry.error_code.as_deref(), Some("archive_decrypt_failed"));
    assert!(entry.payload_fingerprint.is_none());
    assert!(entry.domains.is_empty());
    // The actor id is optional and the row survives without an operator.
    assert!(entry.actor_user_id.is_none());

    store.pool().close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn audit_page_is_clamped_and_ordered_newest_first() {
    let (repository, store, path) = sqlite_repository().await;
    for index in 0..3 {
        let record = ConfigAuditRecord::success(
            Some(1),
            ConfigAuditAction::Export,
            ConfigBackendKind::Sqlite,
            index,
            "fingerprint",
            Vec::new(),
        );
        repository
            .record_config_audit(&record)
            .await
            .expect("record audit");
    }

    let page = repository
        .list_config_audit(-5, 2)
        .await
        .expect("list audit");
    assert_eq!(page.total, 3);
    assert_eq!(page.first, 0);
    assert_eq!(page.rows, 2);
    assert_eq!(page.entries.len(), 2);
    // Newest first: the last written row is the first entry.
    assert_eq!(page.entries[0].archive_bytes, Some(2));
    assert_eq!(page.entries[1].archive_bytes, Some(1));

    let clamped = repository
        .list_config_audit(0, MAX_AUDIT_PAGE_ROWS + 1_000)
        .await
        .expect("list audit");
    assert_eq!(clamped.rows, MAX_AUDIT_PAGE_ROWS);
    assert_eq!(clamped.entries.len(), 3);

    store.pool().close().await;
    let _ = std::fs::remove_file(path);
}

#[test]
fn sanitized_errors_are_bounded() {
    let message = sanitize_error_message(&format!("line one\nline two {}", "y".repeat(512)));
    assert!(message.chars().count() <= MAX_AUDIT_ERROR_CHARS);
    assert_eq!(
        sanitize_error_message("configuration import failed"),
        "configuration import failed"
    );
}
