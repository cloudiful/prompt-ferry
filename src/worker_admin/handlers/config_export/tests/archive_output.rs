//! Archive and metadata behavior of the administrator configuration export.
//!
//! Asserts the administered happy path end to end: the response is a sealed
//! archive, it decrypts back to the seeded configuration, and the metadata
//! endpoint reports the same payload fingerprint without carrying any secret.

use super::*;
use axum::body::to_bytes;
use tower::ServiceExt;

use crate::db::config_repository::archive::decode_archive;
use crate::worker_admin;

#[tokio::test]
async fn admin_export_returns_a_decryptable_archive_and_metadata_summary() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "export-admin", true).await;
    let endpoint_id = seed_configuration(&state).await;
    let app = worker_admin::router(state.clone());

    let response = app
        .clone()
        .oneshot(export_request(Some("export-admin"), PASSPHRASE))
        .await
        .expect("export request");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/octet-stream")
    );
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store, no-cache, must-revalidate, private")
    );
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .expect("content disposition")
        .to_string();
    assert!(disposition.starts_with("attachment; filename=\"prompt-ferry-config-sqlite-"));
    assert!(!disposition.contains(PASSPHRASE));
    assert_eq!(
        response
            .headers()
            .get("x-config-export-backend")
            .and_then(|value| value.to_str().ok()),
        Some("sqlite")
    );
    let fingerprint_header = response
        .headers()
        .get("x-config-export-fingerprint")
        .and_then(|value| value.to_str().ok())
        .expect("fingerprint header")
        .to_string();
    assert_eq!(fingerprint_header.len(), 64);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("archive body");
    let decoded = decode_archive(PASSPHRASE, &body).expect("archive decrypts");
    assert_eq!(decoded.manifest.payload_fingerprint, fingerprint_header);
    let exported = decoded
        .domains
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("endpoint exported");
    assert_eq!(exported.api_key.as_deref(), Some("endpoint-secret"));
    assert!(
        decoded
            .domains
            .users
            .iter()
            .all(|user| user.password_hash.is_some()),
        "password hashes are exported"
    );
    assert!(
        !String::from_utf8_lossy(&body).contains(PASSPHRASE),
        "the archive never carries the passphrase"
    );

    let metadata = app
        .oneshot(metadata_request("export-admin"))
        .await
        .expect("metadata request");
    assert_eq!(metadata.status(), StatusCode::OK);
    let metadata_body: serde_json::Value = serde_json::from_slice(
        &to_bytes(metadata.into_body(), usize::MAX)
            .await
            .expect("metadata body"),
    )
    .expect("metadata json");
    assert_eq!(metadata_body["payload_fingerprint"], fingerprint_header);
    assert_eq!(metadata_body["backend_kind"], "sqlite");
    assert_eq!(metadata_body["format_version"], 1);
    let users_domain = metadata_body["domains"]
        .as_array()
        .expect("domains")
        .iter()
        .find(|domain| domain["name"] == "users")
        .expect("users domain");
    assert_eq!(users_domain["records"], 1);

    close_state(store, path).await;
}
