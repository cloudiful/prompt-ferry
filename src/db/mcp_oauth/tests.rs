//! PostgreSQL personal MCP OAuth storage tests (issue #1208 P2).
//!
//! The tests connect through the dedicated test target guard and the issue's
//! owned `pfy_test_*` schema created for P2. Every fixture is deleted before
//! the pool closes; the schema itself is retained for P3 and the final audit.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::db::config_repository::ConfigRepository;
use crate::db::{
    McpOAuthAuthorizationState, McpOAuthGrant, McpOAuthRefreshFence, McpOAuthRegistration,
    McpOAuthStateInput, McpOAuthStoredCredentials, McpServerInput,
};
use crate::keys::hash_client_key;
use crate::relay_secrets::RelaySecretManager;

/// The task-owned schema prepared for issue #1208 P2 (see #change-28234).
const OWNED_SCHEMA: &str = "pfy_test_79116da1ea8b41ff8b82abad4becd8fe";

/// The actual P2 migration pair, so the reversal regression exercises the
/// shipped DDL instead of a rewritten copy.
const MCP_OAUTH_UP: &str = include_str!("../../../migrations/20261105170618_mcp_oauth.up.sql");
const MCP_OAUTH_DOWN: &str = include_str!("../../../migrations/20261105170618_mcp_oauth.down.sql");

#[allow(dead_code)]
mod db_harness {
    pub mod test_pg_target {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/test_pg_target.rs"
        ));
    }
}

fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([11_u8; 32])).expect("manager")
}

async fn owned_pool() -> Option<PgPool> {
    scoped_pool(OWNED_SCHEMA).await
}

async fn scoped_pool(schema: &str) -> Option<PgPool> {
    let target = db_harness::test_pg_target::resolve_target().expect("dedicated test target");
    let target = target?;
    let options = target.connect_options().options([("search_path", schema)]);
    Some(
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .expect("connect scoped schema"),
    )
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
                "client_id": "pg-client-id",
                "client_secret": "pg-client-secret",
                "client_auth_method": "client_secret_basic",
                "redirect_uri": "https://app.example.test/callback",
                "resource": "https://mcp.example.test/mcp?login",
                "issuer": "https://issuer.example.test",
                "metadata": {
                    "authorization_endpoint": "https://issuer.example.test/authorize",
                    "token_endpoint": "https://issuer.example.test/token",
                    "registration_endpoint": "https://issuer.example.test/register"
                }
            }),
        },
        credentials: McpOAuthStoredCredentials {
            engine: serde_json::json!({
                "client_id": "pg-client-id",
                "token_response": {
                    "access_token": "pg-access-token",
                    "refresh_token": "pg-refresh-token",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "vendor_nested": { "id_token": "pg-vendor-id-token" }
                },
                "granted_scopes": ["read-mcp"],
                "token_received_at": 1_760_000_001_i64,
                "issuer": "https://issuer.example.test"
            }),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            scopes: vec!["read-mcp".to_string()],
            issuer: Some("https://issuer.example.test".to_string()),
        },
    }
}

async fn create_fixture(pool: &PgPool) -> (i64, Uuid) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (login_name, password_hash, display_name) \
         VALUES ($1, 'hash', $1) RETURNING user_id",
    )
    .bind(format!("oauth-user-{}", Uuid::new_v4().simple()))
    .fetch_one(pool)
    .await
    .expect("insert user");
    let server_id = Uuid::new_v4();
    let created = ConfigRepository::postgres(pool)
        .create_mcp_server(
            server_id,
            oauth_server_input(
                &format!("oauth-server-{}", Uuid::new_v4().simple()),
                user_id,
            ),
        )
        .await
        .expect("create oauth server");
    (user_id, created.server_id)
}

async fn remove_fixture(pool: &PgPool, user_id: i64, server_id: Uuid) {
    let _ = sqlx::query("DELETE FROM mcp_servers WHERE server_id = $1")
        .bind(server_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM users WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
}

#[tokio::test]
async fn postgres_grant_round_trips_encrypted_and_trigger_revokes() {
    let Some(pool) = owned_pool().await else {
        return;
    };
    let (user_id, server_id) = create_fixture(&pool).await;
    let repository = ConfigRepository::postgres(&pool);

    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    assert_eq!(installed.generation, 1);

    let row = sqlx::query(
        "SELECT registration_ciphertext, credentials_ciphertext \
         FROM mcp_oauth_credentials WHERE server_id = $1",
    )
    .bind(server_id)
    .fetch_one(&pool)
    .await
    .expect("credential row");
    let registration: Vec<u8> = row.get("registration_ciphertext");
    let credentials: Vec<u8> = row.get("credentials_ciphertext");
    for plaintext in [
        b"pg-client-secret".as_slice(),
        b"pg-access-token",
        b"pg-refresh-token",
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
        serde_json::json!("pg-client-id")
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
        loaded.credentials.engine["token_response"]["access_token"],
        serde_json::json!("pg-access-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_response"]["refresh_token"],
        serde_json::json!("pg-refresh-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_response"]["vendor_nested"]["id_token"],
        serde_json::json!("pg-vendor-id-token")
    );
    assert_eq!(
        loaded.credentials.engine["token_received_at"],
        serde_json::json!(1_760_000_001_i64)
    );

    // A name and timeout edit keeps the OAuth rows.
    sqlx::query("UPDATE mcp_servers SET name = $1, timeout_ms = 45000 WHERE server_id = $2")
        .bind(format!("renamed-{}", Uuid::new_v4().simple()))
        .bind(server_id)
        .execute(&pool)
        .await
        .expect("name edit");
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, user_id)
            .await
            .expect("status after name edit")
            .is_some()
    );

    // Direct SQL changing the URL must revoke at the trigger boundary.
    sqlx::query("UPDATE mcp_servers SET url = $1 WHERE server_id = $2")
        .bind("https://changed.example.test/mcp")
        .bind(server_id)
        .execute(&pool)
        .await
        .expect("url edit");
    assert!(
        repository
            .mcp_oauth_credential_status(server_id, user_id)
            .await
            .expect("status after url edit")
            .is_none()
    );

    remove_fixture(&pool, user_id, server_id).await;
    pool.close().await;
}

#[tokio::test]
async fn postgres_state_is_single_use_and_refresh_fence_rejects_stale_writers() {
    let Some(pool) = owned_pool().await else {
        return;
    };
    let (user_id, server_id) = create_fixture(&pool).await;
    let repository = ConfigRepository::postgres(&pool);
    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");

    let session_hash = hash_client_key("pg-session");
    let state_hash = hash_client_key("pg-state");
    repository
        .create_mcp_oauth_state(
            &manager(),
            &McpOAuthStateInput {
                state_key_hash: state_hash.clone(),
                session_binding_hash: session_hash.clone(),
                server_id,
                owner_user_id: user_id,
                connection_id: installed.connection_id,
                config_fingerprint: installed.config_fingerprint.clone(),
                redirect_uri: "https://app.example.test/callback".to_string(),
                expires_at: Utc::now() + Duration::minutes(10),
                authorization: McpOAuthAuthorizationState {
                    snapshot: serde_json::json!({
                        "pkce_verifier": "pg-verifier",
                        "csrf_token": "pg-csrf",
                        "expected_issuer": "https://issuer.example.test",
                        "require_issuer": true,
                        "created_at": 1_760_000_001_i64,
                        "requested_scopes": ["read-mcp"]
                    }),
                },
                pending_client: None,
            },
        )
        .await
        .expect("create state");
    assert!(
        repository
            .consume_mcp_oauth_state(&manager(), &state_hash, user_id, "wrong-session")
            .await
            .expect("wrong session")
            .is_none()
    );
    let consumed = repository
        .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
        .await
        .expect("consume")
        .expect("consumed state");
    assert_eq!(
        consumed.authorization.snapshot["csrf_token"],
        serde_json::json!("pg-csrf")
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
        serde_json::json!(1_760_000_001_i64)
    );
    assert!(
        repository
            .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
            .await
            .expect("replay")
            .is_none()
    );

    let lease_owner = Uuid::new_v4();
    let lease = repository
        .claim_mcp_oauth_lease(server_id, user_id, lease_owner, Duration::seconds(60))
        .await
        .expect("claim")
        .expect("lease");
    assert_eq!(lease.connection_id, installed.connection_id);

    let mut refreshed = grant().credentials;
    refreshed.engine["token_response"]["access_token"] = serde_json::json!("pg-access-token-2");
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
    assert!(
        repository
            .save_mcp_oauth_refresh(&manager(), server_id, user_id, fence, &refreshed)
            .await
            .expect("stale generation")
            .is_none()
    );

    let reinstalled = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("reinstall");
    assert_ne!(reinstalled.connection_id, installed.connection_id);
    assert!(
        repository
            .save_mcp_oauth_refresh(&manager(), server_id, user_id, fence, &refreshed)
            .await
            .expect("old epoch")
            .is_none()
    );

    remove_fixture(&pool, user_id, server_id).await;
    pool.close().await;
}

async fn plain_fixture_snapshot(pool: &PgPool, server_id: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT users.login_name || '|' || users.display_name || '|' || \
                mcp_servers.name || '|' || mcp_servers.auth_mode || '|' || \
                coalesce(mcp_servers.url, '') || '|' || \
                cast(mcp_servers.timeout_ms AS TEXT) \
         FROM users JOIN mcp_servers ON mcp_servers.owner_user_id = users.user_id \
         WHERE mcp_servers.server_id = $1",
    )
    .bind(server_id)
    .fetch_one(pool)
    .await
    .expect("read plain fixture snapshot")
}

async fn auth_mode_constraint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conrelid = 'mcp_servers'::regclass \
           AND conname = 'ck_mcp_server_auth_mode'",
    )
    .fetch_one(pool)
    .await
    .expect("read auth-mode constraint")
}

async fn oauth_table_exists(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, Option<String>>("SELECT to_regclass('mcp_oauth_credentials')::TEXT")
        .fetch_one(pool)
        .await
        .expect("read OAuth table presence")
        .is_some()
}

/// Issue #1208 P2V: the shipped 20261105170618 migration pair must be
/// reversible on a disposable owned schema. The down migration refuses to run
/// while live OAuth configuration exists (and the failed attempt is rolled
/// back without deleting data), then after controlled cleanup it drops only
/// the OAuth objects and restores the prior auth-mode constraint; the up
/// migration restores the current structure. Non-OAuth data never changes.
#[tokio::test]
async fn postgres_oauth_migration_down_and_up_are_reversible() {
    let Some(base) = owned_pool().await else {
        return;
    };
    let schema = format!("pfy_test_{}", Uuid::new_v4().simple());
    // `schema` is generated as pfy_test_<32 hex>, so this identifier is safe.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&base)
        .await
        .expect("create disposable schema");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "COMMENT ON SCHEMA {schema} IS 'Task-owned prompt-ferry issue1208 P2V migration reversal fixture; dropped after this test'"
    )))
    .execute(&base)
    .await
    .expect("comment disposable schema");

    let Some(pool) = scoped_pool(&schema).await else {
        drop(base);
        return;
    };
    crate::db::migrate(&pool)
        .await
        .expect("apply the full migration set to the disposable schema");

    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (login_name, password_hash, display_name) \
         VALUES ($1, 'hash', $1) RETURNING user_id",
    )
    .bind(format!("p2v-user-{}", Uuid::new_v4().simple()))
    .fetch_one(&pool)
    .await
    .expect("insert user");
    let plain_server = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO mcp_servers \
         (server_id, scope, owner_user_id, name, transport, url, auth_mode, timeout_ms) \
         VALUES ($1, 'user', $2, $3, 'http', 'https://plain.example.test/mcp', 'none', 30000)",
    )
    .bind(plain_server)
    .bind(user_id)
    .bind(format!("p2v-plain-{}", Uuid::new_v4().simple()))
    .execute(&pool)
    .await
    .expect("insert non-OAuth server");
    let plain_before = plain_fixture_snapshot(&pool, plain_server).await;

    let oauth_server = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO mcp_servers \
         (server_id, scope, owner_user_id, name, transport, url, auth_mode, timeout_ms) \
         VALUES ($1, 'user', $2, $3, 'http', 'https://oauth.example.test/mcp', 'oauth', 30000)",
    )
    .bind(oauth_server)
    .bind(user_id)
    .bind(format!("p2v-oauth-{}", Uuid::new_v4().simple()))
    .execute(&pool)
    .await
    .expect("insert OAuth server");
    sqlx::query(
        "INSERT INTO mcp_oauth_credentials \
         (server_id, owner_user_id, connection_id, generation, config_fingerprint, \
          registration_ciphertext, registration_nonce, registration_key_version, \
          credentials_ciphertext, credentials_nonce, credentials_key_version) \
         VALUES ($1, $2, $3, 1, 'fingerprint', $4, $5, 1, $6, $7, 1)",
    )
    .bind(oauth_server)
    .bind(user_id)
    .bind(Uuid::new_v4())
    .bind(vec![1_u8, 2, 3])
    .bind(vec![4_u8; 12])
    .bind(vec![5_u8, 6, 7])
    .bind(vec![8_u8; 12])
    .execute(&pool)
    .await
    .expect("insert OAuth credential");

    // The down migration must fail while a live oauth-mode server exists.
    // Run it in a transaction so the partial statements cannot delete data.
    let mut tx = pool.begin().await.expect("reversal transaction");
    let failed = sqlx::raw_sql(MCP_OAUTH_DOWN).execute(&mut *tx).await;
    assert!(
        failed.is_err(),
        "down must fail while live OAuth configuration exists"
    );
    tx.rollback().await.expect("roll back failed down");
    assert!(oauth_table_exists(&pool).await);
    let still_stored: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_oauth_credentials")
        .fetch_one(&pool)
        .await
        .expect("count OAuth credentials after rollback");
    assert_eq!(still_stored, 1, "the failed down must not delete data");
    assert!(auth_mode_constraint(&pool).await.contains("oauth"));

    // Controlled cleanup of the OAuth configuration, then a real down.
    sqlx::query("DELETE FROM mcp_servers WHERE server_id = $1")
        .bind(oauth_server)
        .execute(&pool)
        .await
        .expect("delete OAuth server");
    sqlx::raw_sql(MCP_OAUTH_DOWN)
        .execute(&pool)
        .await
        .expect("down after OAuth cleanup");
    assert!(!oauth_table_exists(&pool).await);
    let narrowed = auth_mode_constraint(&pool).await;
    assert!(
        !narrowed.contains("oauth"),
        "down must restore the prior auth-mode constraint: {narrowed}"
    );
    assert_eq!(
        plain_before,
        plain_fixture_snapshot(&pool, plain_server).await
    );

    // Up restores the current structure without touching non-OAuth data.
    sqlx::raw_sql(MCP_OAUTH_UP)
        .execute(&pool)
        .await
        .expect("up migration");
    assert!(oauth_table_exists(&pool).await);
    assert!(auth_mode_constraint(&pool).await.contains("oauth"));
    assert_eq!(
        plain_before,
        plain_fixture_snapshot(&pool, plain_server).await
    );

    pool.close().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&base)
    .await
    .expect("drop disposable schema");
    base.close().await;
}

#[tokio::test]
async fn postgres_active_state_tracks_pending_owner_fingerprint_and_expiry() {
    let Some(pool) = owned_pool().await else {
        return;
    };
    let (user_id, server_id) = create_fixture(&pool).await;
    let repository = ConfigRepository::postgres(&pool);
    let installed = repository
        .install_mcp_oauth_grant(&manager(), server_id, user_id, &grant())
        .await
        .expect("install grant");
    let session_hash = hash_client_key("pg-active-session");
    let state_hash = hash_client_key("pg-active-state");
    repository
        .create_mcp_oauth_state(
            &manager(),
            &McpOAuthStateInput {
                state_key_hash: state_hash.clone(),
                session_binding_hash: session_hash.clone(),
                server_id,
                owner_user_id: user_id,
                connection_id: installed.connection_id,
                config_fingerprint: installed.config_fingerprint.clone(),
                redirect_uri: "https://app.example.test/callback".to_string(),
                expires_at: Utc::now() + Duration::minutes(10),
                authorization: McpOAuthAuthorizationState {
                    snapshot: serde_json::json!({ "csrf_token": "pg-active-csrf" }),
                },
                pending_client: None,
            },
        )
        .await
        .expect("create state");

    assert!(
        repository
            .has_active_mcp_oauth_state(server_id, user_id, &installed.config_fingerprint)
            .await
            .expect("active state")
    );
    assert!(
        !repository
            .has_active_mcp_oauth_state(server_id, user_id + 1, &installed.config_fingerprint)
            .await
            .expect("wrong owner")
    );
    assert!(
        !repository
            .has_active_mcp_oauth_state(server_id, user_id, "stale-fingerprint")
            .await
            .expect("stale fingerprint")
    );

    // An expired attempt stops reporting without any delete running.
    sqlx::query("UPDATE mcp_oauth_states SET expires_at = NOW() - INTERVAL '1 second' WHERE state_key_hash = $1")
        .bind(&state_hash)
        .execute(&pool)
        .await
        .expect("expire state");
    assert!(
        !repository
            .has_active_mcp_oauth_state(server_id, user_id, &installed.config_fingerprint)
            .await
            .expect("expired state")
    );

    // A successful consumption ends it too.
    sqlx::query("UPDATE mcp_oauth_states SET expires_at = NOW() + INTERVAL '10 minutes' WHERE state_key_hash = $1")
        .bind(&state_hash)
        .execute(&pool)
        .await
        .expect("renew state");
    assert!(
        repository
            .has_active_mcp_oauth_state(server_id, user_id, &installed.config_fingerprint)
            .await
            .expect("renewed state")
    );
    repository
        .consume_mcp_oauth_state(&manager(), &state_hash, user_id, &session_hash)
        .await
        .expect("consume")
        .expect("consumed");
    assert!(
        !repository
            .has_active_mcp_oauth_state(server_id, user_id, &installed.config_fingerprint)
            .await
            .expect("consumed state")
    );

    remove_fixture(&pool, user_id, server_id).await;
}
