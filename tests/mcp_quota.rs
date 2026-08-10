#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::Utc;
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use sqlx::PgPool;
use uuid::Uuid;

use prompt_ferry::db::{
    McpCredential, McpQuotaGroupInput, QuotaUnit, ReserveOutcome, create_quota_group,
    list_credentials_by_server, pick_credential, reserve_for_credential, settle_reservation,
    sync_credentials_from_tokens,
};

fn test_database_url() -> Option<String> {
    std::env::var(TEST_DATABASE_URL_ENV).ok()
}

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

trait GrantOutcome {
    fn granted(self, label: &str) -> anyhow::Result<prompt_ferry::db::QuotaGrant>;
}

impl GrantOutcome for ReserveOutcome {
    fn granted(self, label: &str) -> anyhow::Result<prompt_ferry::db::QuotaGrant> {
        match self {
            ReserveOutcome::Granted(grant) => Ok(grant),
            ReserveOutcome::BudgetExceeded => anyhow::bail!("{label}: budget exceeded"),
            ReserveOutcome::NoBudget => anyhow::bail!("{label}: no budget"),
        }
    }
}
