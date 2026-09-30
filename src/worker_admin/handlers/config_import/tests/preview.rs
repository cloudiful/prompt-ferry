//! Dry-run preview behavior: read-only, rejection paths, and request shapes.

use super::*;
use axum::body::Body;
use tower::ServiceExt;

use crate::db::config_repository::ConfigBackendKind;
use crate::worker_admin;

const PREVIEW_URI: &str = "/api/v1/admin/config-import/preview";
const IMPORT_URI: &str = "/api/v1/admin/config-import";

#[tokio::test]
async fn preview_is_read_only_and_reports_domain_differences() {
    let (source, source_store, source_path) = test_state().await;
    seed_configuration(&source).await;
    let archive = export_archive(&source).await;

    let (target, target_store, target_path) = test_state().await;
    attach_session(&target, "preview-admin", true).await;
    let before = fingerprint(&target).await;
    let app = worker_admin::router(target.clone());

    let response = app
        .oneshot(import_request(
            PREVIEW_URI,
            Some("preview-admin"),
            PASSPHRASE,
            &archive,
        ))
        .await
        .expect("preview request");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["backend_kind"], "sqlite");
    assert_eq!(
        payload["payload_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );
    assert!(payload["warnings"].is_array());
    let domains = payload["domains"].as_array().expect("domains");
    let endpoints = domains
        .iter()
        .find(|domain| domain["name"] == "endpoints")
        .expect("endpoints domain");
    assert_eq!(endpoints["archive_records"], 1);
    assert_eq!(endpoints["target_records"], 0);
    assert_eq!(endpoints["creates"], 1);
    let users = domains
        .iter()
        .find(|domain| domain["name"] == "users")
        .expect("users domain");
    assert!(users["archive_records"].as_u64().unwrap_or_default() >= 1);

    // A dry run must not change the target configuration.
    assert_eq!(fingerprint(&target).await, before);

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}

#[tokio::test]
async fn preview_rejects_a_wrong_passphrase_and_a_tampered_archive() {
    let (source, source_store, source_path) = test_state().await;
    let archive = export_archive(&source).await;
    let (target, target_store, target_path) = test_state().await;
    attach_session(&target, "preview-admin", true).await;
    let app = worker_admin::router(target.clone());

    let wrong = app
        .clone()
        .oneshot(import_request(
            PREVIEW_URI,
            Some("preview-admin"),
            "wrong-import-passphrase",
            &archive,
        ))
        .await
        .expect("preview request");
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(wrong).await["error"]["code"],
        "archive_decrypt_failed"
    );

    let mut tampered = archive.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    let response = app
        .oneshot(import_request(
            PREVIEW_URI,
            Some("preview-admin"),
            PASSPHRASE,
            &tampered,
        ))
        .await
        .expect("preview request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}

#[tokio::test]
async fn preview_rejects_a_backend_mismatch_and_unknown_request_fields() {
    let (source, source_store, source_path) = test_state().await;
    let mut snapshot = decode(&export_archive(&source).await);
    snapshot.manifest.backend_kind = ConfigBackendKind::Postgres;
    let postgres_archive = seal(snapshot, ConfigBackendKind::Postgres);

    let (target, target_store, target_path) = test_state().await;
    attach_session(&target, "preview-admin", true).await;
    let app = worker_admin::router(target.clone());

    let mismatch = app
        .clone()
        .oneshot(import_request(
            PREVIEW_URI,
            Some("preview-admin"),
            PASSPHRASE,
            &postgres_archive,
        ))
        .await
        .expect("preview request");
    assert_eq!(mismatch.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_body(mismatch).await["error"]["code"],
        "archive_backend_mismatch"
    );

    // `deny_unknown_fields` rejects a body the contract does not define.
    let unknown = axum::http::Request::builder()
        .method("POST")
        .uri(PREVIEW_URI)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, "prompt_ferry_session=preview-admin")
        .body(Body::from(
            serde_json::json!({
                "passphrase": PASSPHRASE,
                "archive_base64": "",
                "unexpected": true,
            })
            .to_string(),
        ))
        .expect("request");
    let response = app.oneshot(unknown).await.expect("preview request");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}

#[tokio::test]
async fn import_rejects_an_unsupported_sqlite_domain() {
    let (source, source_store, source_path) = test_state().await;
    let mut snapshot = decode(&export_archive(&source).await);
    // SQLite cannot store per-user redaction rules; a payload that carries
    // them must be rejected rather than silently dropped.
    snapshot.domains.user_redaction_configs =
        vec![crate::db::config_repository::UserRedactionConfigSnapshot {
            user_id: 1,
            config: serde_json::json!({ "pattern": "secret" }),
        }];
    let archive = seal(snapshot, ConfigBackendKind::Sqlite);

    let (target, target_store, target_path) = test_state().await;
    attach_session(&target, "import-admin", true).await;
    let before = fingerprint(&target).await;
    let app = worker_admin::router(target.clone());

    let response = app
        .oneshot(import_request(
            IMPORT_URI,
            Some("import-admin"),
            PASSPHRASE,
            &archive,
        ))
        .await
        .expect("import request");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "config_import_unsupported_domain"
    );
    assert_eq!(fingerprint(&target).await, before);

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}
