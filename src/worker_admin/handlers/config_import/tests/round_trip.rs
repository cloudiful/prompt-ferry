//! Committed import and rollback behavior.

use super::*;
use tower::ServiceExt;

use crate::db::config_repository::ConfigBackendKind;
use crate::worker_admin;

const IMPORT_URI: &str = "/api/v1/admin/config-import";

#[tokio::test]
async fn import_restores_configuration_into_a_fresh_instance() {
    let (source, source_store, source_path) = test_state().await;
    seed_configuration(&source).await;
    let archive = export_archive(&source).await;
    let source_snapshot = decode(&archive);

    let (target, target_store, target_path) = test_state().await;
    attach_session(&target, "import-admin", true).await;
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
    let status = response.status();
    let payload = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "import failed: {payload}");
    assert_eq!(
        payload["payload_fingerprint"],
        source_snapshot.manifest.payload_fingerprint
    );

    let restored = snapshot(&target).await;

    let admin = restored
        .domains
        .users
        .iter()
        .find(|user| user.login_name == "admin")
        .expect("admin restored");
    assert!(crate::keys::verify_password(
        "admin-password",
        admin.password_hash.as_deref().expect("password hash"),
    ));

    let endpoint = restored
        .domains
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == "imported upstream")
        .expect("endpoint restored");
    assert_eq!(endpoint.api_key.as_deref(), Some("endpoint-secret"));
    assert_eq!(
        endpoint.proxy_url.as_deref(),
        Some("http://proxy.example:8080")
    );
    assert_eq!(endpoint.admin_api_key.as_deref(), Some("admin-key"));
    assert_eq!(
        endpoint
            .oauth
            .as_ref()
            .map(|oauth| oauth.access_token.as_str()),
        Some("oauth-access")
    );

    assert!(
        restored
            .domains
            .client_keys
            .iter()
            .any(|key| key.secret.is_some()),
        "the client key secret is restored"
    );

    // The export projects bearer tokens into `mcp_credentials`; the import must
    // rebuild the server's token array with labels, positions and enable flags.
    let server = restored
        .domains
        .mcp_servers
        .iter()
        .find(|server| server.name == "imported-mcp")
        .expect("mcp server restored");
    let mut credentials: Vec<_> = restored
        .domains
        .mcp_credentials
        .iter()
        .filter(|credential| credential.server_id == server.server_id)
        .collect();
    credentials.sort_by_key(|credential| credential.position);
    assert_eq!(credentials.len(), 2);
    assert_eq!(credentials[0].secret, "bearer-one");
    assert!(credentials[0].enabled);
    assert_eq!(credentials[1].secret, "bearer-two");
    assert!(!credentials[1].enabled);

    assert!(
        restored
            .domains
            .settings
            .iter()
            .any(|setting| setting.key == "redaction_config"),
        "the worker setting is restored"
    );

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}

#[tokio::test]
async fn import_rolls_back_when_a_write_fails() {
    let (source, source_store, source_path) = test_state().await;
    seed_configuration(&source).await;
    let mut snapshot = decode(&export_archive(&source).await);
    // A duplicate login violates the users unique constraint after the
    // transaction has already deleted and re-inserted rows, so the whole
    // import must roll back.
    let mut duplicate = snapshot.domains.users[0].clone();
    duplicate.user_id = 9_999;
    snapshot.domains.users.push(duplicate);
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
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"]["code"],
        "config_import_write_failed"
    );
    assert_eq!(
        fingerprint(&target).await,
        before,
        "a failed import writes nothing"
    );

    let admin = target
        .user_store
        .get_user_password_by_login("admin")
        .await
        .expect("load admin")
        .expect("admin survived the rollback");
    assert!(crate::keys::verify_password(
        "admin-password",
        &admin.password_hash
    ));

    close_state(source_store, source_path).await;
    close_state(target_store, target_path).await;
}
