//! Archive and metadata behavior of the administrator configuration export.
//!
//! Asserts the administered happy path end to end: the response is a sealed
//! archive, it decrypts back to the seeded configuration, and the metadata
//! endpoint reports the same payload fingerprint without carrying any secret.

use super::harness::manager;
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

/// A personal MCP OAuth grant and a pending authorization must never leave the
/// process through the configuration export, while the ordinary server
/// metadata still does.
#[tokio::test]
async fn admin_export_excludes_personal_oauth_secrets_and_runtime_payloads() {
    const CLIENT_SECRET: &str = "SENTINEL-OAUTH-CLIENT-SECRET";
    const ACCESS_TOKEN: &str = "SENTINEL-OAUTH-ACCESS-TOKEN";
    const REFRESH_TOKEN: &str = "SENTINEL-OAUTH-REFRESH-TOKEN";
    const ID_TOKEN: &str = "SENTINEL-OAUTH-ID-TOKEN";
    const PKCE_VERIFIER: &str = "SENTINEL-OAUTH-PKCE-VERIFIER";

    let (state, store, path) = test_state().await;
    attach_session(&state, "export-admin", true).await;
    let endpoint_id = seed_configuration(&state).await;

    let server_id = Uuid::new_v4();
    state
        .config_repository
        .create_mcp_server(
            server_id,
            db::McpServerInput {
                scope: "user".to_string(),
                owner_user_id: Some(1),
                source_endpoint_id: None,
                name: "exported oauth server".to_string(),
                aggregate_naming_mode: "qualified_only".to_string(),
                transport: "http".to_string(),
                provider_kind: None,
                url: Some("https://mcp.example.test/mcp".to_string()),
                command: None,
                args: serde_json::json!([]),
                env_json: serde_json::json!({}),
                bearer_tokens_json: serde_json::json!([]),
                http_headers_json: serde_json::json!({}),
                auth_mode: "oauth".to_string(),
                basic_username: None,
                basic_password: None,
                proxy_url: None,
                tool_filter_mode: "blacklist".to_string(),
                allowed_tools: serde_json::json!([]),
                disabled_tools: serde_json::json!([]),
                disabled_resources: serde_json::json!([]),
                enabled: true,
                timeout_ms: 30_000,
                lifecycle_policy: "auto".to_string(),
                lifecycle_manual_protocol_version: None,
            },
        )
        .await
        .expect("create oauth server");
    let grant = db::McpOAuthGrant {
        registration: db::McpOAuthRegistration {
            snapshot: serde_json::json!({
                "client_id": "export-client-id",
                "client_secret": CLIENT_SECRET,
                "client_auth_method": "client_secret_basic",
                "redirect_uri": "https://app.example.test/callback",
                "resource": "https://mcp.example.test/mcp",
                "issuer": "https://issuer.example.test",
                "metadata": {
                    "authorization_endpoint": "https://issuer.example.test/authorize",
                    "token_endpoint": "https://issuer.example.test/token"
                }
            }),
        },
        credentials: db::McpOAuthStoredCredentials {
            engine: serde_json::json!({
                "client_id": "export-client-id",
                "token_response": {
                    "access_token": ACCESS_TOKEN,
                    "refresh_token": REFRESH_TOKEN,
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "vendor_nested": { "id_token": ID_TOKEN }
                },
                "granted_scopes": ["read-mcp"],
                "token_received_at": 1_760_000_001_i64,
                "issuer": "https://issuer.example.test"
            }),
            scopes: vec!["read-mcp".to_string()],
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            issuer: Some("https://issuer.example.test".to_string()),
        },
    };
    let installed = state
        .config_repository
        .install_mcp_oauth_grant(&manager(), server_id, 1, &grant)
        .await
        .expect("install oauth grant");
    state
        .config_repository
        .create_mcp_oauth_state(
            &manager(),
            &db::McpOAuthStateInput {
                state_key_hash: crate::keys::hash_client_key("export-state"),
                session_binding_hash: crate::keys::hash_client_key("export-session"),
                server_id,
                owner_user_id: 1,
                connection_id: installed.connection_id,
                config_fingerprint: installed.config_fingerprint.clone(),
                redirect_uri: "https://app.example.test/callback".to_string(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
                authorization: db::McpOAuthAuthorizationState {
                    snapshot: serde_json::json!({
                        "pkce_verifier": PKCE_VERIFIER,
                        "csrf_token": "export-csrf",
                        "expected_issuer": "https://issuer.example.test",
                        "require_issuer": true,
                        "created_at": 1_760_000_001_i64,
                        "requested_scopes": ["read-mcp"]
                    }),
                },
                pending_client: Some(db::McpOAuthRegistration {
                    snapshot: serde_json::json!({
                        "client_id": "export-client-id",
                        "client_secret": CLIENT_SECRET,
                        "client_auth_method": "client_secret_basic",
                        "redirect_uri": "https://app.example.test/callback",
                        "resource": "https://mcp.example.test/mcp",
                        "metadata": {
                            "authorization_endpoint": "https://issuer.example.test/authorize",
                            "token_endpoint": "https://issuer.example.test/token"
                        }
                    }),
                }),
            },
        )
        .await
        .expect("create pending state");

    let app = worker_admin::router(state.clone());
    let response = app
        .clone()
        .oneshot(export_request(Some("export-admin"), PASSPHRASE))
        .await
        .expect("export request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("archive body");
    let decoded = decode_archive(PASSPHRASE, &body).expect("archive decrypts");

    let exported_domains = serde_json::to_string(&decoded.domains).expect("serialize domains");
    let exported_manifest = serde_json::to_string(&decoded.manifest).expect("serialize manifest");
    let raw_body = String::from_utf8_lossy(&body);
    for secret in [
        CLIENT_SECRET,
        ACCESS_TOKEN,
        REFRESH_TOKEN,
        ID_TOKEN,
        PKCE_VERIFIER,
    ] {
        assert!(
            !exported_domains.contains(secret),
            "the decoded export domains must not carry {secret}"
        );
        assert!(
            !exported_manifest.contains(secret),
            "the export manifest must not carry {secret}"
        );
        assert!(
            !raw_body.contains(secret),
            "the sealed archive must not carry {secret}"
        );
    }

    // The ordinary configuration and the non-secret OAuth server metadata are
    // still exported: this is a real export, not an empty archive.
    let endpoint = decoded
        .domains
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("endpoint exported");
    assert_eq!(endpoint.api_key.as_deref(), Some("endpoint-secret"));
    let server = decoded
        .domains
        .mcp_servers
        .iter()
        .find(|server| server.server_id == server_id)
        .expect("oauth server metadata exported");
    assert_eq!(server.auth_mode, "oauth");
    assert_eq!(server.scope, "user");
    assert_eq!(server.owner_user_id, Some(1));
    assert_eq!(server.url.as_deref(), Some("https://mcp.example.test/mcp"));

    let metadata = app
        .oneshot(metadata_request("export-admin"))
        .await
        .expect("metadata request");
    assert_eq!(metadata.status(), StatusCode::OK);
    let metadata_body = to_bytes(metadata.into_body(), usize::MAX)
        .await
        .expect("metadata body");
    let metadata_text = String::from_utf8_lossy(&metadata_body);
    let metadata_json: serde_json::Value =
        serde_json::from_slice(&metadata_body).expect("metadata json");
    let metadata_domains = metadata_json["domains"].to_string();
    for secret in [
        CLIENT_SECRET,
        ACCESS_TOKEN,
        REFRESH_TOKEN,
        ID_TOKEN,
        PKCE_VERIFIER,
    ] {
        assert!(
            !metadata_text.contains(secret) && !metadata_domains.contains(secret),
            "the export metadata must not carry {secret}"
        );
    }

    close_state(store, path).await;
}
