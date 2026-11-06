//! ConfigRepository personal MCP OAuth tests (issue #1208 P2).
//!
//! These tests prove the storage-boundary behaviour end to end on the
//! encrypted standalone SQLite backend: encrypted round trip without any
//! secret export, owner isolation, single-use state consumption, refresh
//! fencing across connection epochs, and trigger-driven revocation.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::db::config_repository::ConfigRepository;
use crate::db::{
    McpOAuthAuthorizationState, McpOAuthGrant, McpOAuthRefreshFence, McpOAuthRegistration,
    McpOAuthStateInput, McpOAuthStoredCredentials, McpServerInput,
};
use crate::keys::hash_client_key;
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::StandaloneConfigStore;

fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([9_u8; 32])).expect("manager")
}

async fn open_repository() -> (Arc<StandaloneConfigStore>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("prompt-ferry-oauth-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    (store, path)
}

async fn close_repository(store: Arc<StandaloneConfigStore>, path: std::path::PathBuf) {
    let pool = store.pool().clone();
    drop(store);
    pool.close().await;
    let _ = std::fs::remove_file(path);
}

fn next_user_id() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

async fn create_user(store: &StandaloneConfigStore, user_id: i64) {
    sqlx::query(
        "INSERT INTO standalone_users \
         (user_id, login_name, display_name, password_hash, is_admin, enabled) \
         VALUES (?1, ?2, ?2, 'hash', 0, 1)",
    )
    .bind(user_id)
    .bind(format!("oauth-user-{user_id}"))
    .execute(store.pool())
    .await
    .expect("insert user");
}

fn oauth_server_input(name: &str, owner: i64) -> McpServerInput {
    McpServerInput {
        scope: "user".to_string(),
        owner_user_id: Some(owner),
        source_endpoint_id: None,
        name: name.to_string(),
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
    }
}

fn grant() -> McpOAuthGrant {
    McpOAuthGrant {
        registration: McpOAuthRegistration {
            snapshot: serde_json::json!({
                "client_id": "client-id",
                "client_secret": "client-secret",
                "client_auth_method": "client_secret_basic",
                "redirect_uri": "https://app.example.test/callback",
                "resource": "https://mcp.example.test/mcp?login",
                "issuer": "https://issuer.example.test",
                "metadata": {
                    "authorization_endpoint": "https://issuer.example.test/authorize",
                    "token_endpoint": "https://issuer.example.test/token",
                    "registration_endpoint": "https://issuer.example.test/register",
                    "scopes_supported": ["read-mcp"]
                }
            }),
        },
        credentials: McpOAuthStoredCredentials {
            engine: serde_json::json!({
                "client_id": "client-id",
                "token_response": {
                    "access_token": "access-token",
                    "refresh_token": "refresh-token",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "read-mcp",
                    "vendor_nested": { "id_token": "vendor-id-token", "region": "eu" }
                },
                "granted_scopes": ["read-mcp"],
                "token_received_at": 1_760_000_000_i64,
                "issuer": "https://issuer.example.test"
            }),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            scopes: vec!["read-mcp".to_string()],
            issuer: Some("https://issuer.example.test".to_string()),
        },
    }
}

fn authorization() -> McpOAuthAuthorizationState {
    McpOAuthAuthorizationState {
        snapshot: serde_json::json!({
            "pkce_verifier": "pkce-verifier",
            "csrf_token": "csrf-state",
            "expected_issuer": "https://issuer.example.test",
            "require_issuer": true,
            "created_at": 1_760_000_000_i64,
            "requested_scopes": ["read-mcp"]
        }),
    }
}

async fn setup() -> (
    ConfigRepository,
    Arc<StandaloneConfigStore>,
    std::path::PathBuf,
    i64,
    Uuid,
) {
    let (store, path) = open_repository().await;
    let user_id = next_user_id();
    create_user(&store, user_id).await;
    let repository = ConfigRepository::sqlite(store.clone(), manager());
    let server_id = Uuid::new_v4();
    repository
        .create_mcp_server(server_id, oauth_server_input("personal", user_id))
        .await
        .expect("create oauth server");
    (repository, store, path, user_id, server_id)
}

#[tokio::test]
async fn sqlite_grant_round_trip_is_encrypted_lossless_and_redacted() {
    let (repository, store, path, user_id, server_id) = setup().await;

    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    assert_eq!(installed.generation, 1);
    assert_eq!(installed.scopes, vec!["read-mcp".to_string()]);
    assert_eq!(
        installed.issuer.as_deref(),
        Some("https://issuer.example.test")
    );

    let row = sqlx::query(
        "SELECT registration_ciphertext, credentials_ciphertext \
         FROM standalone_mcp_oauth_credentials WHERE server_id = ?1",
    )
    .bind(server_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("credential row");
    let registration: Vec<u8> = row.get("registration_ciphertext");
    let credentials: Vec<u8> = row.get("credentials_ciphertext");
    for plaintext in [
        b"client-secret".as_slice(),
        b"access-token",
        b"refresh-token",
    ] {
        assert!(
            !registration
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
        assert!(
            !credentials
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );
    }

    let loaded = repository
        .load_mcp_oauth_credential(&manager(), server_id, user_id)
        .await
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(
        loaded.registration.snapshot["client_id"],
        serde_json::json!("client-id")
    );
    assert_eq!(
        loaded.registration.snapshot["client_secret"],
        serde_json::json!("client-secret")
    );
    assert_eq!(
        loaded.registration.snapshot["client_auth_method"],
        serde_json::json!("client_secret_basic")
    );
    assert_eq!(
        loaded.registration.snapshot["resource"],
        serde_json::json!("https://mcp.example.test/mcp?login")
    );
    assert_eq!(
        loaded.registration.snapshot["metadata"]["registration_endpoint"],
        serde_json::json!("https://issuer.example.test/register")
    );
    assert_eq!(
        loaded.credentials.engine["token_response"]["access_token"],
        serde_json::json!("access-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_response"]["refresh_token"],
        serde_json::json!("refresh-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_response"]["vendor_nested"]["id_token"],
        serde_json::json!("vendor-id-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_received_at"],
        serde_json::json!(1_760_000_000_i64)
    );
    assert_eq!(
        loaded.credentials.engine["client_id"],
        serde_json::json!("client-id")
    );

    let status = repository
        .mcp_oauth_credential_status(server_id, user_id)
        .await
        .expect("status")
        .expect("stored status");
    assert_eq!(status.connection_id, installed.connection_id);
    assert_eq!(status.generation, installed.generation);

    let grant = grant();
    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant)
        .await
        .expect("install grant");
    let session_hash = hash_client_key("session");
    let state_key = "lossless-state";
    let state_hash = hash_client_key(state_key);
    let registration = grant.registration.clone();
    let input = McpOAuthStateInput {
        state_key_hash: state_hash.clone(),
        session_binding_hash: session_hash.clone(),
        server_id,
        owner_user_id: user_id,
        connection_id: installed.connection_id,
        config_fingerprint: installed.config_fingerprint.clone(),
        redirect_uri: "https://app.example.test/callback".to_string(),
        expires_at: Utc::now() + Duration::minutes(10),
        authorization: authorization(),
        pending_client: Some(registration.clone()),
    };
    repository
        .create_mcp_oauth_state(&manager(), &input)
        .await
        .expect("create state");

    let secrets = repository
        .load_mcp_oauth_credential(&manager(), server_id, user_id)
        .await
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(secrets.registration.snapshot, grant.registration.snapshot);
    assert_eq!(secrets.credentials.engine, grant.credentials.engine);

    let consumed = repository
        .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
        .await
        .expect("consume")
        .expect("consumed state");
    assert_eq!(
        consumed.authorization.snapshot, input.authorization.snapshot,
        "the full authorization engine snapshot must round-trip"
    );
    assert_eq!(
        consumed
            .pending_client
            .as_ref()
            .map(|value| &value.snapshot),
        Some(&registration.snapshot),
        "the pending confidential client registration must round-trip"
    );

    // Every secret-bearing container must redact distinctive sentinels.
    let rendered = [
        format!("{:?}", grant.registration),
        format!("{:?}", grant.credentials),
        format!("{:?}", input.authorization),
        format!("{:?}", grant),
        format!("{:?}", secrets),
        format!("{:?}", input),
        format!("{:?}", consumed),
    ]
    .join("\n");
    for sentinel in [
        "client-secret",
        "access-token",
        "refresh-token",
        "vendor-id-token",
        "pkce-verifier",
        "csrf-state",
    ] {
        assert!(
            !rendered.contains(sentinel),
            "redacted Debug leaked {sentinel}"
        );
    }

    assert!(
        repository
            .clear_mcp_oauth_credential(server_id, user_id)
            .await
            .expect("clear")
    );
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, user_id)
            .await
            .expect("status after clear")
            .is_none()
    );

    close_repository(store, path).await;
}

#[tokio::test]
async fn sqlite_cross_owner_access_is_denied() {
    let (repository, store, path, user_id, server_id) = setup().await;
    let other_user = user_id + 1000;
    create_user(&store, other_user).await;
    repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");

    assert!(
        repository
            .load_mcp_oauth_credential(&manager(), server_id, other_user)
            .await
            .is_err()
    );
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, other_user)
            .await
            .is_err()
    );
    assert!(
        repository
            .install_mcp_oauth_grant(&manager(), server_id, other_user, &grant())
            .await
            .is_err()
    );
    assert!(
        repository
            .clear_mcp_oauth_credential(server_id, other_user)
            .await
            .is_err()
    );

    close_repository(store, path).await;
}

#[tokio::test]
async fn sqlite_state_is_single_use_and_binding_checked() {
    let (repository, store, path, user_id, server_id) = setup().await;
    repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    let status = repository
        .mcp_oauth_credential_status(server_id, user_id)
        .await
        .expect("status")
        .expect("credential");
    let session = "session-cookie";
    let session_hash = hash_client_key(session);
    let state_key = "state-key";
    let state_hash = hash_client_key(state_key);
    let input = McpOAuthStateInput {
        state_key_hash: state_hash.clone(),
        session_binding_hash: session_hash.clone(),
        server_id,
        owner_user_id: user_id,
        connection_id: status.connection_id,
        config_fingerprint: status.config_fingerprint.clone(),
        redirect_uri: "https://app.example.test/callback".to_string(),
        expires_at: Utc::now() + Duration::minutes(10),
        authorization: authorization(),
        pending_client: None,
    };
    repository
        .create_mcp_oauth_state(&manager(), &input)
        .await
        .expect("create state");

    let binding = repository
        .peek_mcp_oauth_state(&state_hash)
        .await
        .expect("peek")
        .expect("binding");
    assert_eq!(binding.server_id, server_id);
    assert_eq!(binding.owner_user_id, user_id);
    assert_eq!(binding.connection_id, status.connection_id);

    assert!(
        repository
            .consume_mcp_oauth_state(&manager(), &state_hash, user_id, "wrong-session")
            .await
            .expect("wrong session")
            .is_none()
    );
    assert!(
        repository
            .peek_mcp_oauth_state(&state_hash)
            .await
            .expect("still present")
            .is_some()
    );

    let consumed = repository
        .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
        .await
        .expect("consume")
        .expect("consumed state");
    assert_eq!(
        consumed.authorization.snapshot["csrf_token"],
        serde_json::json!("csrf-state")
    );
    assert_eq!(
        consumed.authorization.snapshot["expected_issuer"],
        serde_json::json!("https://issuer.example.test")
    );
    assert_eq!(
        consumed.authorization.snapshot["require_issuer"],
        serde_json::json!(true)
    );
    assert_eq!(
        consumed.authorization.snapshot["requested_scopes"],
        serde_json::json!(["read-mcp"])
    );
    assert_eq!(
        consumed.authorization.snapshot["created_at"],
        serde_json::json!(1_760_000_000_i64)
    );
    assert_eq!(consumed.binding.session_binding_hash, session_hash);

    assert!(
        repository
            .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
            .await
            .expect("replay")
            .is_none()
    );
    assert!(
        repository
            .peek_mcp_oauth_state(&state_hash)
            .await
            .expect("gone")
            .is_none()
    );

    let mut expired = input.clone();
    expired.state_key_hash = hash_client_key("expired-state");
    expired.expires_at = Utc::now() - Duration::seconds(1);
    repository
        .create_mcp_oauth_state(&manager(), &expired)
        .await
        .expect("create expired state");
    assert!(
        repository
            .peek_mcp_oauth_state(&expired.state_key_hash)
            .await
            .expect("peek expired")
            .is_none()
    );
    assert!(
        repository
            .consume_mcp_oauth_state(&manager(), &expired.state_key_hash, user_id, &session_hash)
            .await
            .expect("consume expired")
            .is_none()
    );

    close_repository(store, path).await;
}

#[tokio::test]
async fn sqlite_refresh_fence_rejects_stale_epochs_and_generations() {
    let (repository, store, path, user_id, server_id) = setup().await;
    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    let lease_owner = Uuid::new_v4();
    let lease = repository
        .claim_mcp_oauth_lease(server_id, user_id, lease_owner, Duration::seconds(60))
        .await
        .expect("claim")
        .expect("lease");
    assert_eq!(lease.connection_id, installed.connection_id);
    assert_eq!(lease.generation, installed.generation);

    let mut refreshed = grant().credentials;
    refreshed.engine["token_response"]["access_token"] = serde_json::json!("access-token-2");
    let fence = McpOAuthRefreshFence {
        connection_id: installed.connection_id,
        generation: installed.generation,
        lease_owner,
    };
    let saved = repository
        .save_mcp_oauth_refresh(&manager(), server_id, user_id, fence, &refreshed)
        .await
        .expect("save refresh")
        .expect("fenced refresh");
    assert_eq!(saved.generation, installed.generation + 1);

    let stale_generation = repository
        .save_mcp_oauth_refresh(&manager(), server_id, user_id, fence, &refreshed)
        .await
        .expect("stale generation");
    assert!(stale_generation.is_none());

    let wrong_owner = McpOAuthRefreshFence {
        lease_owner: Uuid::new_v4(),
        ..fence
    };
    assert!(
        repository
            .save_mcp_oauth_refresh(&manager(), server_id, user_id, wrong_owner, &refreshed)
            .await
            .expect("wrong lease owner")
            .is_none()
    );

    assert!(
        repository
            .release_mcp_oauth_lease(server_id, user_id, fence)
            .await
            .expect("release")
    );

    // A fresh grant installs a new connection epoch; the old fence can no
    // longer overwrite the stored credentials.
    let reinstalled = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("reinstall grant");
    assert_ne!(reinstalled.connection_id, installed.connection_id);
    assert!(
        repository
            .save_mcp_oauth_refresh(&manager(), server_id, user_id, fence, &refreshed)
            .await
            .expect("stale epoch")
            .is_none()
    );

    close_repository(store, path).await;
}

#[tokio::test]
async fn sqlite_config_revocation_trigger_clears_oauth_but_name_edit_retains() {
    let (repository, store, path, user_id, server_id) = setup().await;
    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    let session_hash = hash_client_key("session");
    let state_hash = hash_client_key("revocation-state");
    repository
        .create_mcp_oauth_state(
            &manager(),
            &McpOAuthStateInput {
                state_key_hash: state_hash.clone(),
                session_binding_hash: session_hash,
                server_id,
                owner_user_id: user_id,
                connection_id: installed.connection_id,
                config_fingerprint: installed.config_fingerprint.clone(),
                redirect_uri: "https://app.example.test/callback".to_string(),
                expires_at: Utc::now() + Duration::minutes(10),
                authorization: authorization(),
                pending_client: None,
            },
        )
        .await
        .expect("create state");
    repository
        .claim_mcp_oauth_lease(server_id, user_id, Uuid::new_v4(), Duration::seconds(60))
        .await
        .expect("claim")
        .expect("lease");

    sqlx::query("UPDATE standalone_mcp_servers SET name = ?1 WHERE server_id = ?2")
        .bind("renamed-oauth")
        .bind(server_id.to_string())
        .execute(store.pool())
        .await
        .expect("rename");
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, user_id)
            .await
            .expect("status after rename")
            .is_some(),
        "a name-only edit must retain the grant"
    );
    assert!(
        repository
            .peek_mcp_oauth_state(&state_hash)
            .await
            .expect("peek after rename")
            .is_some(),
        "a name-only edit must retain pending state"
    );
    assert!(
        repository
            .claim_mcp_oauth_lease(server_id, user_id, Uuid::new_v4(), Duration::seconds(60),)
            .await
            .expect("claim after rename")
            .is_none(),
        "a name-only edit must retain the refresh lease"
    );

    sqlx::query("UPDATE standalone_mcp_servers SET url = ?1 WHERE server_id = ?2")
        .bind("https://other.example.test/mcp")
        .bind(server_id.to_string())
        .execute(store.pool())
        .await
        .expect("change url");
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, user_id)
            .await
            .expect("status after url change")
            .is_none(),
        "a critical config change must clear the grant"
    );
    assert!(
        repository
            .peek_mcp_oauth_state(&state_hash)
            .await
            .expect("peek after url change")
            .is_none(),
        "a critical config change must clear pending state"
    );
    let leases: i64 = sqlx::query("SELECT COUNT(*) AS count FROM standalone_mcp_oauth_leases")
        .fetch_one(store.pool())
        .await
        .expect("lease count")
        .get("count");
    assert_eq!(leases, 0, "a critical config change must clear leases");

    close_repository(store, path).await;
}
