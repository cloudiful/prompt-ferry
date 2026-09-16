#[path = "support/db_harness.rs"]
mod db_harness;

use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use sqlx::PgPool;
use uuid::Uuid;

use prompt_ferry::db::{
    OverviewBucket, OverviewWindow, RequestRecordCategory, RequestRecordQuery,
    list_credentials_by_server, list_request_records, request_records_overview,
    sync_credentials_from_tokens,
};

async fn insert_mcp_server(pool: &PgPool, name: &str) -> Uuid {
    let server_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mcp_servers(
            server_id, scope, owner_user_id, name, transport, url, command, args, env_json,
            bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools,
            disabled_tools, disabled_resources, aggregate_naming_mode, enabled, timeout_ms
        ) VALUES ($1, 'admin', NULL, $2, 'http', NULL, NULL, '[]', '{}', '[]', '{}',
            'blacklist', '[]', '[]', '[]', 'passthrough_preferred', TRUE, 30000)"#,
    )
    .bind(server_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("insert mcp server");
    server_id
}

#[tokio::test]
async fn sync_credentials_reconciles_token_array_positions() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp credential test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let server_id = insert_mcp_server(pool, "sync-server").await;
    sync_credentials_from_tokens(pool, server_id, &serde_json::json!(["one", "two"])).await?;
    let credentials = list_credentials_by_server(pool, server_id).await?;
    assert_eq!(credentials.len(), 2);
    assert_eq!(credentials[0].secret, "one");
    assert_eq!(credentials[1].secret, "two");

    sync_credentials_from_tokens(
        pool,
        server_id,
        &serde_json::json!([
            { "token": "one-changed", "enabled": false },
            "two",
            "three"
        ]),
    )
    .await?;
    let credentials = list_credentials_by_server(pool, server_id).await?;
    assert_eq!(credentials.len(), 3);
    assert_eq!(credentials[0].secret, "one-changed");
    assert!(!credentials[0].enabled);
    assert_eq!(credentials[2].secret, "three");

    schema.cleanup().await?;
    Ok(())
}

async fn insert_mcp_server_with_provider(pool: &PgPool, name: &str, provider_kind: &str) -> Uuid {
    let server_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mcp_servers(
            server_id, scope, owner_user_id, name, provider_kind, transport, url, command, args,
            env_json, bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools,
            disabled_tools, disabled_resources, aggregate_naming_mode, enabled, timeout_ms
        ) VALUES ($1, 'admin', NULL, $2, $3, 'http', 'https://example.test/mcp', NULL, '[]',
            '{}', '[]', '{}', 'blacklist', '[]', '[]', '[]', 'passthrough_preferred', TRUE,
            30000)"#,
    )
    .bind(server_id)
    .bind(name)
    .bind(provider_kind)
    .execute(pool)
    .await
    .expect("insert mcp server with provider");
    server_id
}

async fn insert_mcp_request_record(
    pool: &PgPool,
    server_id: Uuid,
    server_name: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO request_records(
            request_id, path, request_storage_mode, created_at, request_has_previous_response_id,
            conversation_source, event_kind, request_state, updated_at, storage_sanitized,
            storage_sanitized_nul_count, request_category, route_selection_reason,
            http_request_compressed, redaction_applied, redaction_findings_count,
            redaction_replacements_count, upstream_redaction_enabled, response_capture_truncated,
            mcp_server_id, mcp_server_name, ok
        ) VALUES ($1, '/mcp', 'full', NOW(), FALSE, 'direct', 'request', 'completed', NOW(), TRUE,
            0, 'mcp', 'default', FALSE, FALSE, 0, 0, FALSE, FALSE, $2, $3, TRUE)"#,
    )
    .bind(Uuid::new_v4())
    .bind(server_id)
    .bind(server_name)
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn mcp_overview_exposes_provider_dimension_and_unit() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping; set {TEST_DATABASE_URL_ENV}");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = schema.pool.clone();

    let firecrawl = insert_mcp_server_with_provider(&pool, "firecrawl-server", "firecrawl").await;
    let context7 = insert_mcp_server_with_provider(&pool, "context7-server", "context7").await;
    insert_mcp_request_record(&pool, firecrawl, "firecrawl-server").await?;
    insert_mcp_request_record(&pool, context7, "context7-server").await?;

    let overview = request_records_overview(
        &pool,
        None,
        RequestRecordCategory::Mcp,
        OverviewWindow {
            start: None,
            end: None,
            bucket: OverviewBucket::Day,
        },
        None,
    )
    .await?;

    let firecrawl_row = overview
        .breakdown
        .iter()
        .find(|row| row.mcp_server_id == Some(firecrawl))
        .expect("firecrawl breakdown row");
    assert_eq!(
        firecrawl_row.server_provider_kind.as_deref(),
        Some("firecrawl")
    );
    assert_eq!(firecrawl_row.usage_unit.as_deref(), Some("credits"));

    let context7_row = overview
        .breakdown
        .iter()
        .find(|row| row.mcp_server_id == Some(context7))
        .expect("context7 breakdown row");
    assert_eq!(
        context7_row.server_provider_kind.as_deref(),
        Some("context7")
    );
    assert_eq!(context7_row.usage_unit.as_deref(), Some("requests"));

    let page = list_request_records(
        &pool,
        RequestRecordQuery {
            request_category: RequestRecordCategory::Mcp,
            rows: 50,
            sort_field: "created_at".to_string(),
            sort_order: -1,
            ..Default::default()
        },
    )
    .await?;
    let firecrawl_record = page
        .records
        .iter()
        .find(|record| record.mcp_server_id == Some(firecrawl))
        .expect("firecrawl request record");
    assert_eq!(
        firecrawl_record.server_provider_kind.as_deref(),
        Some("firecrawl")
    );
    let context7_record = page
        .records
        .iter()
        .find(|record| record.mcp_server_id == Some(context7))
        .expect("context7 request record");
    assert_eq!(
        context7_record.server_provider_kind.as_deref(),
        Some("context7")
    );

    schema.cleanup().await?;
    Ok(())
}
