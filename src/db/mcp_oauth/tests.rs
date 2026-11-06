//! PostgreSQL personal MCP OAuth storage tests.
//!
//! Every run creates its own short-lived strict `pfy_test_<32hex>` schema on
//! the guarded dedicated test target, applies the real migration set once, and
//! drops the schema even when an assertion or later setup step fails. Each
//! created schema carries a per-run ownership marker that is verified before
//! the exact schema is dropped, so cleanup never touches an unknown schema. No
//! manually provisioned schema is required.

use std::any::Any;
use std::panic::{AssertUnwindSafe, resume_unwind};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use futures::FutureExt;
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

/// The shipped migration pair is embedded, so the reversal regression
/// exercises the deployed DDL instead of a rewritten copy.
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

#[derive(Clone)]
struct OwnedSchema {
    name: String,
    database: String,
    token: String,
    purpose: String,
    oid: i64,
}

enum OwnershipCheck {
    Absent,
    Owned(OwnedSchema),
    Mismatch(String),
}

fn ownership_comment(token: &str, purpose: &str) -> String {
    format!("prompt-ferry portable OAuth test fixture: {purpose}; owner-token={token}")
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn strict_schema_name(name: &str) -> bool {
    let suffix = name.strip_prefix("pfy_test_");
    matches!(suffix, Some(suffix) if suffix.len() == 32 && suffix.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
}

async fn current_database(base: &PgPool) -> Result<String, String> {
    sqlx::query_scalar("SELECT current_database()")
        .fetch_one(base)
        .await
        .map_err(|error| format!("read current database failed: {error}"))
}

async fn check_ownership(
    base: &PgPool,
    name: &str,
    token: &str,
    purpose: &str,
    expected_oid: Option<i64>,
    expected_database: &str,
) -> Result<OwnershipCheck, String> {
    if !strict_schema_name(name) {
        return Ok(OwnershipCheck::Mismatch(format!(
            "schema name {name} is not a strict pfy_test_ identifier"
        )));
    }
    let expected_comment = ownership_comment(token, purpose);
    let row = sqlx::query(
        "SELECT n.oid::BIGINT AS oid, obj_description(n.oid, 'pg_namespace') AS comment \
         FROM pg_namespace n WHERE n.nspname = $1",
    )
    .bind(name)
    .fetch_optional(base)
    .await
    .map_err(|error| format!("read schema {name} ownership failed: {error}"))?;
    let Some(row) = row else {
        return Ok(OwnershipCheck::Absent);
    };
    let oid: i64 = row
        .try_get("oid")
        .map_err(|error| format!("read schema {name} oid failed: {error}"))?;
    let comment: Option<String> = row
        .try_get("comment")
        .map_err(|error| format!("read schema {name} comment failed: {error}"))?;
    if comment.as_deref() != Some(expected_comment.as_str()) {
        return Ok(OwnershipCheck::Mismatch(format!(
            "comment {comment:?} does not match the ownership marker for token {token}"
        )));
    }
    if let Some(expected) = expected_oid
        && expected != oid
    {
        return Ok(OwnershipCheck::Mismatch(format!(
            "schema oid {oid} does not match the recorded oid {expected}"
        )));
    }
    let database = current_database(base).await?;
    if database != expected_database {
        return Ok(OwnershipCheck::Mismatch(format!(
            "database {database} does not match the recorded database {expected_database}"
        )));
    }
    Ok(OwnershipCheck::Owned(OwnedSchema {
        name: name.to_string(),
        database,
        token: token.to_string(),
        purpose: purpose.to_string(),
        oid,
    }))
}

async fn create_owned_schema(
    base: &PgPool,
    name: &str,
    token: &str,
    purpose: &str,
) -> Result<OwnedSchema, String> {
    if !strict_schema_name(name) {
        return Err(format!(
            "refusing to create a schema with unexpected name {name}"
        ));
    }
    let comment = ownership_comment(token, purpose);
    let mut tx = base
        .begin()
        .await
        .map_err(|error| format!("begin schema {name} create transaction failed: {error}"))?;
    let create = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {name}")))
        .execute(&mut *tx)
        .await;
    if let Err(error) = create {
        let rollback = tx.rollback().await;
        return Err(match rollback {
            Ok(()) => format!("CREATE schema {name} failed: {error}; transaction rolled back"),
            Err(rollback_error) => format!(
                "CREATE schema {name} failed: {error}; rollback failed: {rollback_error}; ownership must be re-established before any cleanup"
            ),
        });
    }
    let comment_result = sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "COMMENT ON SCHEMA {name} IS {}",
        sql_literal(&comment)
    )))
    .execute(&mut *tx)
    .await;
    if let Err(error) = comment_result {
        let rollback = tx.rollback().await;
        return Err(match rollback {
            Ok(()) => format!("COMMENT on schema {name} failed: {error}; transaction rolled back"),
            Err(rollback_error) => format!(
                "COMMENT on schema {name} failed: {error}; rollback failed: {rollback_error}; ownership must be re-established before any cleanup"
            ),
        });
    }
    if let Err(error) = tx.commit().await {
        return Err(format!(
            "commit for schema {name} failed: {error}; ownership must be re-established before any cleanup"
        ));
    }
    let database = current_database(base).await.map_err(|error| {
        format!(
            "schema {name} was committed but its database identity could not be read: {error}; ownership unverified"
        )
    })?;
    let ownership = check_ownership(base, name, token, purpose, None, &database)
        .await
        .map_err(|error| {
            format!(
                "schema {name} was committed but ownership could not be checked: {error}; ownership unverified"
            )
        })?;
    match ownership {
        OwnershipCheck::Owned(owned) => {
            eprintln!(
                "mcp_oauth runtime schema created: name={} oid={} purpose={purpose}",
                owned.name, owned.oid
            );
            Ok(owned)
        }
        OwnershipCheck::Absent => Err(format!(
            "schema {name} is absent after a successful create transaction"
        )),
        OwnershipCheck::Mismatch(reason) => Err(format!(
            "schema {name} ownership verification failed after create ({reason}); retained"
        )),
    }
}

async fn cleanup_owned_schema(base: &PgPool, owned: &OwnedSchema) -> Result<(), String> {
    if !strict_schema_name(&owned.name) {
        return Err(format!(
            "refusing to drop a schema with unexpected name {}",
            owned.name
        ));
    }
    match check_ownership(
        base,
        &owned.name,
        &owned.token,
        &owned.purpose,
        Some(owned.oid),
        &owned.database,
    )
    .await?
    {
        OwnershipCheck::Owned(_) => {}
        OwnershipCheck::Absent => return Ok(()),
        OwnershipCheck::Mismatch(reason) => {
            return Err(format!(
                "refusing to drop schema {}: ownership mismatch ({reason})",
                owned.name
            ));
        }
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {} CASCADE",
        owned.name
    )))
    .execute(base)
    .await
    .map_err(|error| format!("drop schema {} failed: {error}", owned.name))?;
    let present =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pg_namespace WHERE nspname = $1")
            .bind(&owned.name)
            .fetch_one(base)
            .await
            .map_err(|error| format!("verify schema {} absence failed: {error}", owned.name))?;
    if present != 0 {
        return Err(format!("schema {} is still present after drop", owned.name));
    }
    eprintln!(
        "mcp_oauth runtime schema cleaned: name={} oid={}",
        owned.name, owned.oid
    );
    Ok(())
}

fn combine_failure(schema: &str, setup_error: &str, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => format!("{setup_error}; owned schema {schema} was cleaned up"),
        Err(cleanup_error) => {
            format!("{setup_error}; cleanup of owned schema {schema} failed: {cleanup_error}")
        }
    }
}

async fn finish_failed_initialization(
    base: &PgPool,
    owned: &OwnedSchema,
    setup_error: String,
) -> String {
    let cleanup = cleanup_owned_schema(base, owned).await;
    combine_failure(&owned.name, &setup_error, cleanup)
}

async fn settle_failed_creation(
    base: &PgPool,
    name: &str,
    token: &str,
    purpose: &str,
    setup_error: &str,
) -> String {
    let database = match current_database(base).await {
        Ok(database) => database,
        Err(error) => {
            return format!(
                "{setup_error}; could not determine the database to verify schema {name}: {error}; retained"
            );
        }
    };
    match check_ownership(base, name, token, purpose, None, &database).await {
        Ok(OwnershipCheck::Owned(owned)) => {
            let cleanup = cleanup_owned_schema(base, &owned).await;
            combine_failure(name, setup_error, cleanup)
        }
        Ok(OwnershipCheck::Absent) => setup_error.to_string(),
        Ok(OwnershipCheck::Mismatch(reason)) => format!(
            "{setup_error}; schema {name} exists but ownership was not re-established ({reason}); retained"
        ),
        Err(error) => {
            format!("{setup_error}; could not verify ownership of schema {name}: {error}; retained")
        }
    }
}

struct RuntimeSchemaFixture {
    owned: OwnedSchema,
    base: PgPool,
    pool: PgPool,
}

impl RuntimeSchemaFixture {
    async fn close(self) -> Result<(), String> {
        self.pool.close().await;
        let cleanup = cleanup_owned_schema(&self.base, &self.owned).await;
        self.base.close().await;
        cleanup.map_err(|error| {
            format!(
                "cleanup of owned schema {} failed: {error}",
                self.owned.name
            )
        })
    }
}

async fn runtime_schema_fixture(purpose: &str, migrate: bool) -> Option<RuntimeSchemaFixture> {
    let target = db_harness::test_pg_target::resolve_target().expect("dedicated test target");
    let target = target?;
    let token = Uuid::new_v4().simple().to_string();
    let schema = format!("pfy_test_{}", Uuid::new_v4().simple());
    let base = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(target.connect_options())
        .await
        .expect("connect base test target");
    let owned = match create_owned_schema(&base, &schema, &token, purpose).await {
        Ok(owned) => owned,
        Err(error) => {
            let failure = settle_failed_creation(&base, &schema, &token, purpose, &error).await;
            base.close().await;
            panic!("runtime schema initialization failed: {failure}");
        }
    };
    let pool = match PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            target
                .connect_options()
                .options([("search_path", owned.name.as_str())]),
        )
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            let failure = finish_failed_initialization(
                &base,
                &owned,
                format!(
                    "connect scoped pool for schema {} failed: {error}",
                    owned.name
                ),
            )
            .await;
            base.close().await;
            panic!("runtime schema initialization failed: {failure}");
        }
    };
    if migrate && let Err(error) = crate::db::migrate(&pool).await {
        pool.close().await;
        let failure = finish_failed_initialization(
            &base,
            &owned,
            format!("migrate schema {} failed: {error}", owned.name),
        )
        .await;
        base.close().await;
        panic!("runtime schema initialization failed: {failure}");
    }
    Some(RuntimeSchemaFixture { owned, base, pool })
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

fn finish_fixture_run(
    schema: &str,
    outcome: Result<(), Box<dyn Any + Send>>,
    cleanup: Result<(), String>,
) {
    match (outcome, cleanup) {
        (Ok(()), Ok(())) => {}
        (Ok(()), Err(error)) => {
            panic!("cleanup failed for runtime schema {schema}: {error}");
        }
        (Err(panic), Ok(())) => resume_unwind(panic),
        (Err(panic), Err(error)) => {
            panic!(
                "cleanup failed for runtime schema {schema}: {error}; original test error: {}",
                panic_message(&panic)
            );
        }
    }
}

async fn panic_cleanup_probe() -> Result<(), String> {
    let Some(fixture) = runtime_schema_fixture("panic cleanup probe", false).await else {
        return Ok(());
    };
    let schema = fixture.owned.name.clone();
    let outcome = AssertUnwindSafe(async {
        panic!("intentional runtime schema cleanup probe");
    })
    .catch_unwind()
    .await;
    let cleanup = fixture.close().await;
    match outcome {
        Err(panic) => {
            let setup = format!(
                "panic cleanup probe for owned schema {schema} failed: {}",
                panic_message(&panic)
            );
            match cleanup {
                Ok(()) => Ok(()),
                Err(error) => Err(combine_failure(&schema, &setup, Err(error))),
            }
        }
        Ok(()) => {
            let setup = format!("panic cleanup probe for owned schema {schema} did not panic");
            Err(combine_failure(&schema, &setup, cleanup))
        }
    }
}

async fn schema_sentinel_present(base: &PgPool, schema: &str) -> bool {
    sqlx::query_scalar::<_, Option<String>>("SELECT to_regclass($1)::TEXT")
        .bind(format!("{schema}.sentinel"))
        .fetch_one(base)
        .await
        .expect("read sentinel presence")
        .is_some()
}

#[tokio::test]
async fn runtime_schema_safety_guards_destructive_cleanup() {
    let Some(fixture) = runtime_schema_fixture("destructive cleanup safety", false).await else {
        return;
    };
    let owned = fixture.owned.clone();
    let schema = owned.name.clone();
    let owned_for_run = owned.clone();
    let schema_for_run = schema.clone();
    let base_for_run = fixture.base.clone();
    let outcome = AssertUnwindSafe(async move {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE {schema_for_run}.sentinel (value INT)"
        )))
        .execute(&base_for_run)
        .await
        .expect("create sentinel");

        // A CREATE collision must never remove the schema that already owns the name.
        let collision_token = Uuid::new_v4().simple().to_string();
        let collision = create_owned_schema(
            &base_for_run,
            &schema_for_run,
            &collision_token,
            "collision",
        )
        .await;
        let collision_error = match collision {
            Ok(_) => panic!("a duplicate schema name must not be created"),
            Err(error) => error,
        };
        let settled = settle_failed_creation(
            &base_for_run,
            &schema_for_run,
            &collision_token,
            "collision",
            &collision_error,
        )
        .await;
        assert!(
            settled.contains(&schema_for_run),
            "failure must name {schema_for_run}: {settled}"
        );
        assert!(
            matches!(
                check_ownership(
                    &base_for_run,
                    &schema_for_run,
                    &owned_for_run.token,
                    "destructive cleanup safety",
                    Some(owned_for_run.oid),
                    &owned_for_run.database,
                )
                .await
                .expect("recheck original ownership"),
                OwnershipCheck::Owned(_)
            ),
            "the original schema must keep its ownership marker"
        );
        assert!(
            schema_sentinel_present(&base_for_run, &schema_for_run).await,
            "failed creation cleanup must not delete the original schema"
        );

        // Cleanup must refuse a record whose ownership token does not match.
        let mut tampered = owned_for_run.clone();
        tampered.token = collision_token.clone();
        let refusal = cleanup_owned_schema(&base_for_run, &tampered)
            .await
            .expect_err("mismatched ownership must refuse cleanup");
        assert!(refusal.contains(&schema_for_run));
        assert!(
            refusal.contains("mismatch"),
            "refusal must explain: {refusal}"
        );
        assert!(
            schema_sentinel_present(&base_for_run, &schema_for_run).await,
            "refused cleanup must not delete the schema"
        );

        // A post-create initialization failure reports the schema and both errors.
        let failure = finish_failed_initialization(
            &base_for_run,
            &tampered,
            "simulated post-create initialization failure".to_string(),
        )
        .await;
        assert!(
            failure.contains(&schema_for_run),
            "failure must name {schema_for_run}: {failure}"
        );
        assert!(failure.contains("simulated post-create initialization failure"));
        assert!(
            failure.contains(&format!("cleanup of owned schema {schema_for_run} failed")),
            "failure must include the guarded cleanup error: {failure}"
        );
        assert!(
            failure.contains("mismatch"),
            "failure must include the specific cleanup cause: {failure}"
        );
        assert!(
            schema_sentinel_present(&base_for_run, &schema_for_run).await,
            "reported initialization failure must not delete the schema"
        );
    })
    .catch_unwind()
    .await;

    let cleanup = fixture.close().await;
    finish_fixture_run(&schema, outcome, cleanup);
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

async fn case_grant_round_trip(pool: PgPool) {
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
}

async fn case_state_and_fence(pool: PgPool) {
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
}

async fn case_active_state(pool: PgPool) {
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

#[tokio::test]
async fn postgres_storage_regressions_share_one_runtime_schema() {
    let Some(fixture) = runtime_schema_fixture("portable OAuth storage regressions", true).await
    else {
        return;
    };
    let schema = fixture.owned.name.clone();
    let pool = fixture.pool.clone();
    let outcome = AssertUnwindSafe(async move {
        panic_cleanup_probe()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        case_grant_round_trip(pool.clone()).await;
        case_state_and_fence(pool.clone()).await;
        case_active_state(pool).await;
    })
    .catch_unwind()
    .await;
    let cleanup = fixture.close().await;
    finish_fixture_run(&schema, outcome, cleanup);
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

/// The shipped 20261105170618 migration pair must be reversible on its own
/// runtime-owned schema. The down migration refuses to run while live OAuth
/// configuration exists (and the failed attempt is rolled back without
/// deleting data), then after controlled cleanup it drops only the OAuth
/// objects and restores the prior auth-mode constraint; the up migration
/// restores the current structure. Non-OAuth data never changes.
#[tokio::test]
async fn postgres_oauth_migration_down_and_up_are_reversible() {
    let Some(fixture) = runtime_schema_fixture("portable OAuth migration reversal", true).await
    else {
        return;
    };
    let schema = fixture.owned.name.clone();
    let pool = fixture.pool.clone();
    let outcome = AssertUnwindSafe(async move {
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
    })
    .catch_unwind()
    .await;
    let cleanup = fixture.close().await;
    finish_fixture_run(&schema, outcome, cleanup);
}
