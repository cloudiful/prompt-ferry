#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::Utc;
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use sqlx::PgPool;
use uuid::Uuid;

use prompt_ferry::db::{
    McpCredential, McpQuotaGroupInput, OverviewBucket, OverviewWindow, QuotaUnit,
    RequestRecordCategory, RequestRecordQuery, ReserveOutcome, create_quota_group,
    list_credentials_by_server, list_request_records, pick_credential, request_records_overview,
    reserve_for_credential, settle_reservation, settle_reservation_with_actual,
    sync_credentials_from_tokens,
};

async fn insert_mcp_server(pool: &PgPool, name: &str) -> Uuid {
    let server_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mcp_servers(
            server_id, scope, owner_user_id, name, transport, url, command, args, env_json,
            bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools,
            disabled_tools, disabled_resources, aggregate_naming_mode, enabled, timeout_ms,
            daily_max_requests, monthly_max_requests
        ) VALUES ($1, 'admin', NULL, $2, 'http', NULL, NULL, '[]', '{}', '[]', '{}',
            'blacklist', '[]', '[]', '[]', 'passthrough_preferred', TRUE, 30000, NULL, NULL)"#,
    )
    .bind(server_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("insert mcp server");
    server_id
}

async fn credential(
    pool: &PgPool,
    server_id: Uuid,
    label: &str,
    secret: &str,
    group_id: Uuid,
) -> McpCredential {
    let position = prompt_ferry::db::list_credentials_by_server(pool, server_id)
        .await
        .unwrap()
        .len() as i32;
    prompt_ferry::db::insert_credential(
        pool,
        server_id,
        label,
        secret,
        position,
        true,
        Some(group_id),
    )
    .await
    .unwrap()
}

async fn credential_with_group(
    pool: &PgPool,
    server_id: Uuid,
    label: &str,
    secret: &str,
    group_id: Uuid,
) -> McpCredential {
    prompt_ferry::db::insert_credential(pool, server_id, label, secret, 0, true, Some(group_id))
        .await
        .unwrap()
}

#[tokio::test]
async fn reservation_commits_and_blocks_after_limit() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "quota-commit".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Requests),
            daily_limit: None,
            monthly_limit: Some(5.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "quota-server").await;
    let credential = credential_with_group(pool, server_id, "a", "secret-a", group.group_id).await;

    let mut grants = Vec::new();
    for index in 0..5 {
        let outcome = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now()).await?;
        let ReserveOutcome::Granted(grant) = outcome else {
            anyhow::bail!("grant {index} must succeed");
        };
        assert_eq!(
            settle_reservation(pool, grant.reservation.request_id, true).await?,
            true
        );
        grants.push(grant);
    }

    let outcome = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now()).await?;
    assert!(matches!(outcome, ReserveOutcome::BudgetExceeded));

    let _ = grants;
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn released_reservations_return_budget() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "quota-release".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Requests),
            daily_limit: None,
            monthly_limit: Some(2.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "quota-server").await;
    let credential = credential_with_group(pool, server_id, "a", "secret-a", group.group_id).await;

    let first = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("first")?;
    let second = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("second")?;
    let blocked = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now()).await?;
    assert!(matches!(blocked, ReserveOutcome::BudgetExceeded));

    assert_eq!(
        settle_reservation(pool, first.reservation.request_id, false).await?,
        true
    );
    let recovered = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("recovered")?;
    assert_eq!(
        settle_reservation(pool, recovered.reservation.request_id, true).await?,
        true
    );
    assert_eq!(
        settle_reservation(pool, second.reservation.request_id, true).await?,
        true
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_reservations_never_exceed_budget() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "quota-concurrent".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Requests),
            daily_limit: None,
            monthly_limit: Some(5.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "quota-server").await;
    let credential = credential_with_group(pool, server_id, "a", "secret-a", group.group_id).await;

    let mut tasks = Vec::new();
    for _ in 0..12 {
        let pool = pool.clone();
        let credential = credential.clone();
        tasks.push(tokio::spawn(async move {
            match reserve_for_credential(&pool, &credential, Uuid::new_v4(), Utc::now()).await {
                Ok(ReserveOutcome::Granted(_)) => 1,
                Ok(ReserveOutcome::BudgetExceeded) | Ok(ReserveOutcome::NoBudget) => 0,
                Err(_) => 0,
            }
        }));
    }
    let granted: usize = futures::future::join_all(tasks)
        .await
        .into_iter()
        .map(|result| result.unwrap())
        .sum();
    assert!(
        granted <= 5,
        "concurrent reservations exceeded budget: {granted}"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn picker_balances_by_group_usage_ratio() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group_a = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "ratio-a".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Requests),
            daily_limit: None,
            monthly_limit: Some(10.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let group_b = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "ratio-b".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Requests),
            daily_limit: None,
            monthly_limit: Some(10.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "ratio-server").await;
    let credential_a = credential(pool, server_id, "token-a", "secret-a", group_a.group_id).await;
    let credential_b = credential(pool, server_id, "token-b", "secret-b", group_b.group_id).await;

    for _ in 0..3 {
        let outcome =
            reserve_for_credential(pool, &credential_a, Uuid::new_v4(), Utc::now()).await?;
        let ReserveOutcome::Granted(grant) = outcome else {
            anyhow::bail!("credential a reservation must succeed");
        };
        assert_eq!(
            settle_reservation(pool, grant.reservation.request_id, true).await?,
            true
        );
    }

    let credentials = list_credentials_by_server(pool, server_id).await?;
    let picked = pick_credential(pool, &credentials, Utc::now(), &[]).await?;
    assert_eq!(
        picked.as_ref().map(|item| item.credential_id),
        Some(credential_b.credential_id)
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn sync_credentials_reconciles_token_array_positions() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let server_id = insert_mcp_server(pool, "sync-server").await;
    sqlx::query(
        r#"INSERT INTO mcp_quota_groups (group_id, name, unit, monthly_limit)
           VALUES (md5('group:' || $1::uuid::text)::uuid, $2, 'requests', 100)"#,
    )
    .bind(server_id)
    .bind("sync-server default")
    .execute(pool)
    .await?;
    sync_credentials_from_tokens(pool, server_id, &serde_json::json!(["one", "two"])).await?;
    let credentials = list_credentials_by_server(pool, server_id).await?;
    assert_eq!(credentials.len(), 2);
    assert_eq!(credentials[0].secret, "one");
    assert_eq!(credentials[1].secret, "two");
    assert!(
        credentials.iter().all(|item| item.quota_group_id.is_some()),
        "migrated tokens must inherit the default quota group"
    );

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
    assert!(
        credentials[2].quota_group_id.is_some(),
        "newly added token must inherit the default quota group"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn settlement_charges_the_actual_units_without_double_counting() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "quota-actual".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Credits),
            daily_limit: Some(100.0),
            monthly_limit: Some(100.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "actual-server").await;
    let credential = credential_with_group(pool, server_id, "a", "secret-a", group.group_id).await;

    // actual < reserved: only the real cost is charged, the rest is released.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("0.25")?;
    assert!(
        settle_reservation_with_actual(pool, grant.reservation.request_id, true, Some(0.25))
            .await?
    );
    assert_accounts(pool, group.group_id, 0.25, 0.0).await?;

    // actual == reserved.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("1.0")?;
    assert!(
        settle_reservation_with_actual(pool, grant.reservation.request_id, true, Some(1.0)).await?
    );
    assert_accounts(pool, group.group_id, 1.25, 0.0).await?;

    // actual > reserved: charged exactly once, never reserved+actual.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("2.0")?;
    assert!(
        settle_reservation_with_actual(pool, grant.reservation.request_id, true, Some(2.0)).await?
    );
    assert_accounts(pool, group.group_id, 3.25, 0.0).await?;

    // Missing actual keeps the reservation as the charge.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("missing")?;
    assert!(settle_reservation_with_actual(pool, grant.reservation.request_id, true, None).await?);
    assert_accounts(pool, group.group_id, 4.25, 0.0).await?;

    // Zero actual also keeps the reservation (only positive values settle).
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("zero")?;
    assert!(
        settle_reservation_with_actual(pool, grant.reservation.request_id, true, Some(0.0)).await?
    );
    assert_accounts(pool, group.group_id, 5.25, 0.0).await?;

    // Failure releases the whole reservation and charges nothing.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("failure")?;
    assert!(
        settle_reservation_with_actual(pool, grant.reservation.request_id, false, Some(9.0))
            .await?
    );
    assert_accounts(pool, group.group_id, 5.25, 0.0).await?;

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn settlement_is_idempotent_and_concurrent_safe() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping mcp quota test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_ferry::db::migrate(&schema.pool).await?;
    let pool = &schema.pool;

    let group = create_quota_group(
        pool,
        McpQuotaGroupInput {
            name: "quota-idempotent".to_string(),
            scope: Some("admin".to_string()),
            owner_user_id: None,
            provider_kind: None,
            unit: Some(QuotaUnit::Credits),
            daily_limit: Some(100.0),
            monthly_limit: Some(100.0),
            default_cost: Some(1.0),
            strict_mode: None,
            billing_period_start: None,
            billing_period_end: None,
        },
    )
    .await?;
    let server_id = insert_mcp_server(pool, "idempotent-server").await;
    let credential = credential_with_group(pool, server_id, "a", "secret-a", group.group_id).await;

    // A second settle of the same request is a no-op.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("idempotent")?;
    let request_id = grant.reservation.request_id;
    assert!(settle_reservation_with_actual(pool, request_id, true, Some(0.5)).await?);
    assert!(!settle_reservation_with_actual(pool, request_id, true, Some(5.0)).await?);
    assert_accounts(pool, group.group_id, 0.5, 0.0).await?;

    // Two concurrent settles of one reservation: exactly one wins.
    let grant = reserve_for_credential(pool, &credential, Uuid::new_v4(), Utc::now())
        .await?
        .granted("concurrent")?;
    let request_id = grant.reservation.request_id;
    let first = {
        let pool = pool.clone();
        tokio::spawn(async move {
            settle_reservation_with_actual(&pool, request_id, true, Some(0.75)).await
        })
    };
    let second = {
        let pool = pool.clone();
        tokio::spawn(async move {
            settle_reservation_with_actual(&pool, request_id, true, Some(0.75)).await
        })
    };
    let first = first.await??;
    let second = second.await??;
    assert_eq!(
        [first, second].into_iter().filter(|won| *won).count(),
        1,
        "exactly one concurrent settlement must win"
    );
    assert_accounts(pool, group.group_id, 1.25, 0.0).await?;

    schema.cleanup().await?;
    Ok(())
}

async fn assert_accounts(
    pool: &PgPool,
    group_id: Uuid,
    expected_used: f64,
    expected_reserved: f64,
) -> anyhow::Result<()> {
    for period_kind in ["day", "month"] {
        let row: (f64, f64) = sqlx::query_as(
            "SELECT used_units, reserved_units FROM mcp_quota_accounts \
             WHERE group_id = $1 AND period_kind = $2",
        )
        .bind(group_id)
        .bind(period_kind)
        .fetch_one(pool)
        .await?;
        assert!(
            (row.0 - expected_used).abs() < 1e-9,
            "{period_kind} used {} != {expected_used}",
            row.0
        );
        assert!(
            (row.1 - expected_reserved).abs() < 1e-9,
            "{period_kind} reserved {} != {expected_reserved}",
            row.1
        );
    }
    Ok(())
}

trait GrantOutcome {
    fn granted(self, label: &str) -> anyhow::Result<prompt_ferry::db::QuotaGrant>;
}

impl GrantOutcome for ReserveOutcome {
    fn granted(self, label: &str) -> anyhow::Result<prompt_ferry::db::QuotaGrant> {
        match self {
            ReserveOutcome::Granted(grant) => Ok(*grant),
            ReserveOutcome::BudgetExceeded => anyhow::bail!("{label}: budget exceeded"),
            ReserveOutcome::NoBudget => anyhow::bail!("{label}: no budget"),
        }
    }
}

async fn insert_mcp_server_with_provider(pool: &PgPool, name: &str, provider_kind: &str) -> Uuid {
    let server_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO mcp_servers(
            server_id, scope, owner_user_id, name, provider_kind, transport, url, command, args,
            env_json, bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools,
            disabled_tools, disabled_resources, aggregate_naming_mode, enabled, timeout_ms,
            daily_max_requests, monthly_max_requests
        ) VALUES ($1, 'admin', NULL, $2, $3, 'http', 'https://example.test/mcp', NULL, '[]',
            '{}', '[]', '{}', 'blacklist', '[]', '[]', '[]', 'passthrough_preferred', TRUE,
            30000, NULL, NULL)"#,
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
