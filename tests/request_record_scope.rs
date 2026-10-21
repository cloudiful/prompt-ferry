// Issue #1012 P1: the MCP dataset scope.
//
// The scope splits the MCP records by record identity, never by path or display
// label: a record reached an MCP service when it carries a server id or a
// stored server name, and it is gateway chatter when it carries neither. The
// fixtures below cover the three service identities (id and name, name only,
// id only for a deleted server) plus the gateway record, and every reader —
// the page, its count, the facets, and the overview — must agree on the same
// selection.
//
// The test owns its connection so the day-bucket assertion is meaningful: the
// session time zone is deliberately not UTC, which is exactly the case the
// pinned UTC truncation has to survive.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};
use prompt_ferry::db::{
    OverviewBucket, OverviewWindow, RequestRecordCategory, RequestRecordMcpScope,
    RequestRecordOverviewPerspective, RequestRecordQuery, list_request_record_facets,
    list_request_records, request_records_overview,
};
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// A user id that no other record uses, so the fixtures are the only rows the
/// scoped queries can see. Every test owns its own id, so the tests stay
/// independent when the runner executes them in parallel.
const FIXTURE_USER_ID_BASE: i64 = 9_000_000_100;

/// The dedicated test database. The test refuses to run without it, so a
/// missing service is a failure rather than a silent skip.
async fn fixture_pool() -> PgPool {
    let target = test_pg_target::resolve_target()
        .expect("the dedicated test target must be valid")
        .expect("the dedicated test database must be configured");
    PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                // A non-UTC session proves the day bucket no longer follows the
                // database session time zone.
                sqlx::query("SET TIME ZONE 'America/New_York'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(target.connect_options())
        .await
        .expect("connect to the confirmed quota test database")
}

struct Fixture {
    request_ids: Vec<Uuid>,
    server_name: String,
    server_id: Uuid,
    deleted_server_id: Uuid,
    user_id: i64,
    created_at: DateTime<Utc>,
}

/// Remove the rows a previous run of this test may have left behind, so a
/// failed run never leaks into the next one. The user id is a dedicated
/// synthetic range, so only this test's own fixtures are affected.
async fn clear_previous_fixtures(pool: &PgPool, user_id: i64) {
    sqlx::query("DELETE FROM request_records WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("clear previous request record fixtures");
    sqlx::query("DELETE FROM mcp_servers WHERE name = $1")
        .bind(format!("scope-fixture-{user_id}"))
        .execute(pool)
        .await
        .expect("clear previous mcp server fixture");
}

async fn insert_record(
    pool: &PgPool,
    request_id: Uuid,
    user_id: i64,
    request_category: RequestRecordCategory,
    path: &str,
    mcp_server_id: Option<Uuid>,
    mcp_server_name: Option<&str>,
    created_at: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO request_records \
         (request_id, user_id, path, request_category, mcp_server_id, mcp_server_name, \
          request_state, ok, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 'completed', TRUE, $7)",
    )
    .bind(request_id)
    .bind(user_id)
    .bind(path)
    .bind(request_category.as_str())
    .bind(mcp_server_id)
    .bind(mcp_server_name)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("insert request record fixture");
}

/// Insert the service and gateway fixtures and return their request ids.
async fn insert_fixtures(pool: &PgPool, user_id: i64) -> Fixture {
    clear_previous_fixtures(pool, user_id).await;
    let created_at = Utc::now();
    let server_id = Uuid::new_v4();
    let deleted_server_id = Uuid::new_v4();
    let server_name = format!("scope-fixture-{user_id}");

    sqlx::query(
        "INSERT INTO mcp_servers (server_id, scope, name, transport, url) \
         VALUES ($1, 'admin', $2, 'http', 'https://example.invalid/mcp')",
    )
    .bind(server_id)
    .bind(&server_name)
    .execute(pool)
    .await
    .expect("insert mcp server fixture");

    let service_with_name = Uuid::new_v4();
    let service_name_only = Uuid::new_v4();
    let service_id_only = Uuid::new_v4();
    let service_named_unknown = Uuid::new_v4();
    let gateway = Uuid::new_v4();
    let ai_record = Uuid::new_v4();

    insert_record(
        pool,
        service_with_name,
        user_id,
        RequestRecordCategory::Mcp,
        "/mcp",
        Some(server_id),
        Some("scope-fixture-server"),
        created_at,
    )
    .await;
    insert_record(
        pool,
        service_name_only,
        user_id,
        RequestRecordCategory::Mcp,
        "/mcp",
        None,
        Some("scope-fixture-server"),
        created_at,
    )
    .await;
    insert_record(
        pool,
        service_id_only,
        user_id,
        RequestRecordCategory::Mcp,
        "/mcp",
        Some(deleted_server_id),
        None,
        created_at,
    )
    .await;
    insert_record(
        pool,
        service_named_unknown,
        user_id,
        RequestRecordCategory::Mcp,
        "/mcp",
        None,
        Some("(unknown)"),
        created_at,
    )
    .await;
    insert_record(
        pool,
        gateway,
        user_id,
        RequestRecordCategory::Mcp,
        "/mcp",
        None,
        None,
        created_at,
    )
    .await;
    insert_record(
        pool,
        ai_record,
        user_id,
        RequestRecordCategory::Ai,
        "/v1/responses",
        None,
        None,
        created_at,
    )
    .await;

    Fixture {
        request_ids: vec![
            service_with_name,
            service_name_only,
            service_id_only,
            service_named_unknown,
            gateway,
            ai_record,
        ],
        server_name,
        server_id,
        deleted_server_id,
        user_id,
        created_at,
    }
}

async fn cleanup(pool: &PgPool, fixture: &Fixture) {
    for request_id in &fixture.request_ids {
        sqlx::query("DELETE FROM request_records WHERE request_id = $1")
            .bind(request_id)
            .execute(pool)
            .await
            .expect("delete request record fixture");
    }
    sqlx::query("DELETE FROM mcp_servers WHERE name = $1")
        .bind(&fixture.server_name)
        .execute(pool)
        .await
        .expect("delete mcp server fixture");
    sqlx::query("DELETE FROM request_records WHERE user_id = $1")
        .bind(fixture.user_id)
        .execute(pool)
        .await
        .expect("delete the remaining request record fixtures");
}

fn scoped_query(scope: RequestRecordMcpScope, user_id: i64) -> RequestRecordQuery {
    RequestRecordQuery {
        visible_user_id: Some(user_id),
        request_category: RequestRecordCategory::Mcp,
        mcp_scope: scope,
        first: 0,
        rows: 50,
        sort_field: "created_at".to_string(),
        sort_order: -1,
        ..RequestRecordQuery::default()
    }
}

fn overview_window(bucket: OverviewBucket) -> OverviewWindow {
    let end = Utc::now() + Duration::hours(1);
    OverviewWindow {
        start: Some(end - Duration::days(1)),
        end: Some(end),
        bucket,
    }
}

#[tokio::test]
async fn the_mcp_scope_splits_service_attempts_from_gateway_chatter() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 1;
    let fixture = insert_fixtures(&pool, user_id).await;

    let service =
        list_request_records(&pool, scoped_query(RequestRecordMcpScope::Service, user_id))
            .await
            .expect("service scope page");
    let gateway =
        list_request_records(&pool, scoped_query(RequestRecordMcpScope::Gateway, user_id))
            .await
            .expect("gateway scope page");
    let all = list_request_records(&pool, scoped_query(RequestRecordMcpScope::All, user_id))
        .await
        .expect("all scope page");

    assert_eq!(
        service.total, 4,
        "the service scope keeps the id-and-name, name-only, id-only, and \
         name-only service records"
    );
    assert_eq!(
        gateway.total, 1,
        "the gateway scope keeps the record without a server identity"
    );
    assert_eq!(all.total, 5, "the all scope keeps every MCP record");
    assert_eq!(
        service.total + gateway.total,
        all.total,
        "the service and the gateway scope partition the MCP records"
    );

    let gateway_records = &gateway.records;
    assert!(
        gateway_records
            .iter()
            .all(|record| record.mcp_server_id.is_none() && record.mcp_server_name.is_none()),
        "the gateway scope must not leak a record that reached a service"
    );
    let service_records = &service.records;
    assert!(
        service_records
            .iter()
            .any(|record| record.mcp_server_name.as_deref() == Some("scope-fixture-server")),
        "the service scope keeps the name-only failed attempt"
    );
    assert!(
        service_records
            .iter()
            .any(|record| record.mcp_server_id.is_some() && record.mcp_server_name.is_none()),
        "the service scope keeps the historical id-only record of a deleted server"
    );

    cleanup(&pool, &fixture).await;
}

#[tokio::test]
async fn the_facets_follow_the_same_scope_as_the_records() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 2;
    let fixture = insert_fixtures(&pool, user_id).await;
    let start = Some(fixture.created_at - Duration::hours(1));
    let end = Some(fixture.created_at + Duration::hours(1));

    let service = list_request_record_facets(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        true,
        start,
        end,
        RequestRecordMcpScope::Service,
    )
    .await
    .expect("service scope facets");
    let gateway = list_request_record_facets(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        true,
        start,
        end,
        RequestRecordMcpScope::Gateway,
    )
    .await
    .expect("gateway scope facets");

    assert!(
        service.models.contains(&"scope-fixture-server".to_string()),
        "the service scope offers the MCP server as a target"
    );
    assert!(
        !gateway.models.contains(&"scope-fixture-server".to_string()),
        "the gateway scope offers no MCP server target"
    );

    cleanup(&pool, &fixture).await;
}

#[tokio::test]
async fn the_overview_follows_the_same_scope_and_keeps_the_scopes_apart() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 3;
    let fixture = insert_fixtures(&pool, user_id).await;
    let window = overview_window(OverviewBucket::Hour);

    let service = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::Service,
    )
    .await
    .expect("service scope overview");
    let gateway = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::Gateway,
    )
    .await
    .expect("gateway scope overview");
    let all = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::All,
    )
    .await
    .expect("all scope overview");

    assert_eq!(service.summary.request_count, 4);
    assert_eq!(gateway.summary.request_count, 1);
    assert_eq!(all.summary.request_count, 5);
    assert_eq!(
        service.trend_granularity, "hour",
        "the server reports the trend granularity it selected"
    );

    // The cached service entry must not answer a gateway request.
    let service_again = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::Service,
    )
    .await
    .expect("cached service scope overview");
    assert_eq!(service_again.summary.request_count, 4);

    cleanup(&pool, &fixture).await;
}

#[tokio::test]
async fn the_overview_breakdown_keeps_the_record_identity_of_every_row() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 6;
    let fixture = insert_fixtures(&pool, user_id).await;
    let window = overview_window(OverviewBucket::Hour);

    let overview = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::All,
    )
    .await
    .expect("all scope overview");

    let gateway_rows: Vec<_> = overview
        .breakdown
        .iter()
        .filter(|row| row.mcp_scope == Some(RequestRecordMcpScope::Gateway))
        .collect();
    assert_eq!(
        gateway_rows.len(),
        1,
        "only the record without any server identity is gateway chatter"
    );
    assert_eq!(gateway_rows[0].mcp_server_id, None);

    let service_rows: Vec<_> = overview
        .breakdown
        .iter()
        .filter(|row| row.mcp_scope == Some(RequestRecordMcpScope::Service))
        .collect();
    assert_eq!(
        service_rows.len(),
        4,
        "the name-only, id-only, id-and-name, and name-only service rows stay \
         on the service side"
    );
    assert!(
        service_rows
            .iter()
            .any(|row| row.mcp_server_id == Some(fixture.server_id)),
        "the id-and-name service row keeps its server identity"
    );
    assert!(
        service_rows
            .iter()
            .any(|row| row.mcp_server_id == Some(fixture.deleted_server_id)),
        "the id-only service row of a deleted server keeps its server identity"
    );
    assert!(
        service_rows
            .iter()
            .any(|row| row.mcp_server_id.is_none() && row.label == "(unknown)"),
        "a service named like the gateway fallback stays a service row"
    );

    let grouped: i64 = overview.breakdown.iter().map(|row| row.request_count).sum();
    let page = list_request_records(&pool, scoped_query(RequestRecordMcpScope::All, user_id))
        .await
        .expect("all scope page");
    assert_eq!(
        grouped, overview.summary.request_count,
        "the breakdown covers the same records as the overview summary"
    );
    assert_eq!(
        page.total, overview.summary.request_count,
        "the page count and the overview agree on the same scope"
    );

    cleanup(&pool, &fixture).await;
}

#[tokio::test]
async fn the_ai_category_ignores_the_mcp_scope() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 4;
    let fixture = insert_fixtures(&pool, user_id).await;
    let window = overview_window(OverviewBucket::Hour);

    let mut query = scoped_query(RequestRecordMcpScope::Service, user_id);
    query.request_category = RequestRecordCategory::Ai;
    let ai_records = list_request_records(&pool, query)
        .await
        .expect("ai scope page");

    assert_eq!(
        ai_records.total, 1,
        "the scope never narrows the AI records"
    );

    let ai_overview = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Ai,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::Service,
    )
    .await
    .expect("ai scope overview");
    assert_eq!(ai_overview.summary.request_count, 1);

    cleanup(&pool, &fixture).await;
}

#[tokio::test]
async fn the_day_trend_bucket_is_a_logical_utc_date() {
    let pool = fixture_pool().await;
    let user_id = FIXTURE_USER_ID_BASE + 5;
    let fixture = insert_fixtures(&pool, user_id).await;
    let window = overview_window(OverviewBucket::Day);

    let overview = request_records_overview(
        &pool,
        Some(user_id),
        RequestRecordCategory::Mcp,
        window,
        None,
        RequestRecordOverviewPerspective::Model,
        RequestRecordMcpScope::All,
    )
    .await
    .expect("day scope overview");

    assert_eq!(overview.trend_granularity, "day");
    let expected = NaiveDate::from_ymd_opt(
        fixture.created_at.date_naive().year(),
        fixture.created_at.month(),
        fixture.created_at.day(),
    )
    .expect("a calendar date always exists")
    .and_hms_opt(0, 0, 0)
    .expect("midnight always exists")
    .and_utc();
    let bucket = overview
        .trend
        .first()
        .expect("the fixture day has a trend bucket")
        .bucket_at;
    assert_eq!(
        bucket, expected,
        "the day bucket stays on the UTC calendar date in a non-UTC session"
    );
    assert_eq!(bucket.hour(), 0, "the day bucket is the UTC midnight");

    cleanup(&pool, &fixture).await;
}
