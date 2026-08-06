use std::{env, str::FromStr};

use prompt_ferry::{db, keys::hash_password};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

const TEST_DATABASE_URL_ENV: &str = "PROMPT_FERRY_TEST_DATABASE_URL";

struct TestSchema {
    pool: PgPool,
    admin_pool: PgPool,
    schema: String,
}

impl TestSchema {
    async fn new() -> anyhow::Result<Self> {
        let database_url = env::var(TEST_DATABASE_URL_ENV)?;
        let schema = format!("pfy_test_{}", Uuid::new_v4().simple());

        let base_options = PgConnectOptions::from_str(&database_url)?;
        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(base_options.clone())
            .await?;
        admin_pool
            .execute(sqlx::AssertSqlSafe(format!(
                r#"CREATE SCHEMA "{}""#,
                schema
            )))
            .await?;

        let schema_options = base_options.options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(schema_options)
            .await?;

        Ok(Self {
            pool,
            admin_pool,
            schema,
        })
    }

    async fn cleanup(&self) -> anyhow::Result<()> {
        self.admin_pool
            .execute(sqlx::AssertSqlSafe(format!(
                r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#,
                self.schema
            )))
            .await?;
        self.pool.close().await;
        self.admin_pool.close().await;
        Ok(())
    }
}

fn test_database_configured() -> bool {
    env::var(TEST_DATABASE_URL_ENV).is_ok()
}

async fn create_user(pool: &PgPool, login_name: &str) -> anyhow::Result<i64> {
    let user = db::create_user(
        pool,
        db::UserCreate {
            login_name: login_name.to_string(),
            password_hash: hash_password("password-123")?,
            display_name: login_name.to_string(),
            is_admin: false,
        },
    )
    .await?;
    Ok(user.user_id)
}

#[tokio::test]
async fn migrate_upgrades_legacy_mcp_servers_table() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;

    schema
        .pool
        .execute(
            r#"
            CREATE TABLE mcp_servers (
                server_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                name TEXT NOT NULL UNIQUE,
                transport TEXT NOT NULL CHECK (transport IN ('http', 'stdio')),
                url TEXT,
                command TEXT,
                args JSONB NOT NULL DEFAULT '[]'::JSONB,
                env_json JSONB NOT NULL DEFAULT '{}'::JSONB,
                bearer_token TEXT NOT NULL DEFAULT '',
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                timeout_ms INTEGER NOT NULL DEFAULT 30000,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    let row = sqlx::query_file!("tests/sql/db_migrations/mcp_servers_scope_column.sql")
        .fetch_one(&schema.pool)
        .await?;

    assert_eq!(row.data_type, "text");
    assert!(row.is_not_null);
    assert!(row.has_default);
    assert!(row.has_owner_constraint);

    let index_exists =
        sqlx::query_file!("tests/sql/db_migrations/mcp_servers_scope_user_index.sql")
            .fetch_one(&schema.pool)
            .await?
            .exists;

    assert!(index_exists);
    let aggregate_naming_mode =
        sqlx::query_file!("tests/sql/db_migrations/mcp_servers_aggregate_naming_mode_column.sql")
            .fetch_one(&schema.pool)
            .await?;

    assert_eq!(aggregate_naming_mode.data_type, "text");
    assert!(aggregate_naming_mode.is_not_null);
    assert!(aggregate_naming_mode.has_default);
    assert!(aggregate_naming_mode.has_mode_constraint);
    let bearer_tokens =
        sqlx::query_file!("tests/sql/db_migrations/mcp_servers_bearer_tokens_column.sql")
            .fetch_one(&schema.pool)
            .await?;

    assert_eq!(bearer_tokens.data_type.as_deref(), Some("jsonb"));
    assert_eq!(bearer_tokens.is_not_null, Some(true));
    assert_eq!(bearer_tokens.has_default, Some(true));
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_adds_content_retention_columns_and_drops_legacy_payload_columns()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let columns = sqlx::query_file!("tests/sql/db_migrations/usage_retention_columns.sql")
        .fetch_one(&schema.pool)
        .await?;
    assert!(columns.content_expired_at_exists);
    assert!(columns.raw_object_key_exists);
    assert!(columns.legacy_payload_columns_removed);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_creates_managed_relays_table() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;

    db::migrate(&schema.pool).await?;

    let row = sqlx::query_file!("tests/sql/db_migrations/managed_relays_table.sql")
        .fetch_one(&schema.pool)
        .await?;

    assert!(row.table_exists.unwrap_or(false));
    assert!(row.relay_id_is_uuid.unwrap_or(false));
    assert!(row.tls_mode_has_check.unwrap_or(false));
    assert!(row.bridge_mode_has_check.unwrap_or(false));
    assert!(row.relay_url_unique.unwrap_or(false));

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_drops_model_route_stream_output_coalescing_override_column() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;

    db::migrate(&schema.pool).await?;

    let row = sqlx::query_file!(
        "tests/sql/db_migrations/model_route_stream_delta_batching_override_removed.sql"
    )
    .fetch_one(&schema.pool)
    .await?;

    assert!(row.missing.unwrap_or(false));

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_upgrades_legacy_usage_events_table() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;

    schema
        .pool
        .execute(
            r#"
            CREATE TABLE users (
                user_id BIGSERIAL PRIMARY KEY,
                login_name TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                display_name TEXT NOT NULL,
                is_admin BOOLEAN NOT NULL DEFAULT FALSE,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE provider_endpoints (
                endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
                owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE usage_events (
                event_id BIGSERIAL PRIMARY KEY,
                request_id UUID NOT NULL,
                user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
                endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL,
                path TEXT NOT NULL,
                model TEXT,
                status INTEGER,
                ok BOOLEAN NOT NULL,
                duration_ms BIGINT NOT NULL,
                first_chunk_ms BIGINT,
                input_tokens BIGINT,
                output_tokens BIGINT,
                total_tokens BIGINT,
                cached_tokens BIGINT,
                cache_read_tokens BIGINT,
                cache_write_tokens BIGINT,
                request_prompt TEXT,
                response_prompt TEXT,
                upstream_error_body TEXT,
                error_code TEXT,
                error_message TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    let row = sqlx::query_file!("tests/sql/db_migrations/usage_events_request_storage_mode.sql")
        .fetch_one(&schema.pool)
        .await?;

    assert_eq!(row.data_type, "text");
    assert!(row.is_not_null);
    assert!(row.has_default);
    assert!(row.has_storage_constraint);

    let timing_columns =
        sqlx::query_file!("tests/sql/db_migrations/request_record_ttft_columns.sql")
            .fetch_one(&schema.pool)
            .await?;
    assert!(timing_columns.ttft_exists);
    assert!(timing_columns.first_chunk_absent);

    let client_key_label_exists =
        sqlx::query_file!("tests/sql/db_migrations/usage_events_client_key_label_exists.sql")
            .fetch_one(&schema.pool)
            .await?
            .exists;

    assert!(client_key_label_exists);
    let request_user_agent_exists = sqlx::query_scalar::<_, bool>(
        r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'request_user_agent'
            )
            "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(request_user_agent_exists);
    let request_state_exists = sqlx::query_scalar::<_, bool>(
        r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'request_state'
            )
            "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(request_state_exists);
    let storage_sanitized_exists = sqlx::query_scalar::<_, bool>(
        r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'storage_sanitized'
            )
            "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(storage_sanitized_exists);
    let storage_sanitized_nul_count_exists = sqlx::query_scalar::<_, bool>(
        r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'storage_sanitized_nul_count'
            )
            "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(storage_sanitized_nul_count_exists);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_upgrades_legacy_client_keys_table() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;

    schema
        .pool
        .execute(
            r#"
            CREATE TABLE users (
                user_id BIGSERIAL PRIMARY KEY,
                login_name TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                display_name TEXT NOT NULL,
                is_admin BOOLEAN NOT NULL DEFAULT FALSE,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;
    schema
        .pool
        .execute(
            r#"
            CREATE TABLE client_keys (
                key_id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                key_prefix TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                last_used_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    let secret_exists = sqlx::query_file!("tests/sql/db_migrations/client_keys_secret_exists.sql")
        .fetch_one(&schema.pool)
        .await?
        .exists;

    assert_eq!(secret_exists, Some(true));

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn visible_mcp_servers_are_scoped_by_owner() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let user_a = create_user(&schema.pool, "user-a").await?;
    let user_b = create_user(&schema.pool, "user-b").await?;

    db::create_mcp_server(
        &schema.pool,
        db::McpServerInput {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "public-server".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            url: Some("https://example.com/mcp".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
        },
    )
    .await?;

    db::create_mcp_server(
        &schema.pool,
        db::McpServerInput {
            scope: "user".to_string(),
            owner_user_id: Some(user_a),
            name: "private-a".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            url: Some("https://example.com/a".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
        },
    )
    .await?;

    db::create_mcp_server(
        &schema.pool,
        db::McpServerInput {
            scope: "user".to_string(),
            owner_user_id: Some(user_b),
            name: "private-b".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            url: Some("https://example.com/b".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({}),
            bearer_tokens_json: serde_json::json!([]),
            http_headers_json: serde_json::json!({}),
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
        },
    )
    .await?;

    let visible_for_none = db::list_visible_mcp_servers(&schema.pool, None).await?;
    let visible_for_a = db::list_visible_mcp_servers(&schema.pool, Some(user_a)).await?;
    let visible_for_b = db::list_visible_mcp_servers(&schema.pool, Some(user_b)).await?;

    assert_eq!(
        visible_for_none
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["public-server"]
    );
    assert_eq!(
        visible_for_a
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["private-a", "public-server"]
    );
    assert_eq!(
        visible_for_b
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        vec!["private-b", "public-server"]
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn migrate_adds_session_load_balancing_contract() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let contract = sqlx::query_file!("tests/sql/db_migrations/session_load_balancing_contract.sql")
        .fetch_one(&schema.pool)
        .await?;

    assert!(contract.route_reason_exists);
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn raw_payload_storage_is_partitioned_and_destructive_migration_drops_old_columns()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let contract = sqlx::query_file!("tests/sql/db_migrations/raw_payloads_contract.sql")
        .fetch_one(&schema.pool)
        .await?;

    assert!(!contract.old_request_raw_column.unwrap_or(false));
    assert!(!contract.old_response_raw_column.unwrap_or(false));
    assert!(contract.raw_parent_is_partitioned.unwrap_or(false));
    assert!(contract.default_partition_exists.unwrap_or(false));
    assert!(contract.overflow_table_exists.unwrap_or(false));
    assert!(contract.event_id_index_exists.unwrap_or(false));
    assert!(contract.created_at_event_id_index_exists.unwrap_or(false));

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn raw_payloads_are_written_and_pruned_without_losing_normalized_only_metadata()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let raw_request_id = Uuid::new_v4();
    let mut raw_record = db::RequestRecordCreate::ai_request(raw_request_id, "/v1/responses");
    raw_record.request_raw_json = Some(serde_json::json!({"input": "raw"}));
    raw_record.response_raw_body = Some("raw response".to_string());
    raw_record.request_conversation_key = Some("raw-conversation".to_string());
    let raw_event_id = db::record_request_record(&schema.pool, raw_record).await?;

    let stored = sqlx::query_file!("tests/sql/raw_payloads_for_event.sql", raw_event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(
        stored.request_raw_json,
        Some(serde_json::json!({"input": "raw"}))
    );
    assert_eq!(stored.response_raw_body.as_deref(), Some("raw response"));
    assert_eq!(
        stored.request_conversation_key.as_deref(),
        Some("raw-conversation")
    );

    let mut raw_update = db::RequestRecordCreate::ai_request(raw_request_id, "/v1/responses");
    raw_update.response_raw_body = Some("updated raw response".to_string());
    let updated_event_id = db::record_request_record(&schema.pool, raw_update).await?;
    assert_eq!(updated_event_id, raw_event_id);
    let updated = sqlx::query_file!("tests/sql/raw_payloads_for_event.sql", raw_event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(
        updated.request_raw_json,
        Some(serde_json::json!({"input": "raw"}))
    );
    assert_eq!(
        updated.response_raw_body.as_deref(),
        Some("updated raw response")
    );

    let mut normalized_only = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    normalized_only.request_conversation_key = Some("normalized-only".to_string());
    let normalized_event_id = db::record_request_record(&schema.pool, normalized_only).await?;

    let initial_report = db::run_raw_payload_maintenance(&schema.pool, 1)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert!(initial_report.partitions_created > 0);
    let normalized_stored =
        sqlx::query_file!("tests/sql/raw_payloads_for_event.sql", normalized_event_id)
            .fetch_one(&schema.pool)
            .await?;
    assert!(normalized_stored.request_raw_json.is_none());
    assert!(normalized_stored.response_raw_body.is_none());
    assert_eq!(
        normalized_stored.request_conversation_key.as_deref(),
        Some("normalized-only")
    );

    sqlx::query_file!("tests/sql/raw_payloads_mark_expired.sql", raw_event_id)
        .execute(&schema.pool)
        .await?;
    let report = db::run_raw_payload_maintenance(&schema.pool, 1)
        .await?
        .expect("raw maintenance should acquire the isolated test lock");
    assert_eq!(report.raw_rows_deleted, 1);

    let pruned = sqlx::query_file!("tests/sql/raw_payloads_for_event.sql", raw_event_id)
        .fetch_one(&schema.pool)
        .await?;
    assert!(pruned.request_raw_json.is_none());
    assert!(pruned.response_raw_body.is_none());
    assert!(pruned.request_conversation_key.is_none());

    let normalized_key = sqlx::query_file!(
        "tests/sql/raw_payloads_conversation_key.sql",
        normalized_event_id
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        normalized_key.request_conversation_key.as_deref(),
        Some("normalized-only")
    );

    schema.cleanup().await?;
    Ok(())
}
