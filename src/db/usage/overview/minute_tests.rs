use super::*;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

async fn insert_fixture_record(
    pool: &sqlx::PgPool,
    request_id: uuid::Uuid,
    user_id: i64,
    at: DateTime<Utc>,
    category: &str,
    server_name: Option<&str>,
    ok: bool,
) {
    let state = if ok { "completed" } else { "failed" };
    sqlx::query_file!(
        "src/sql/usage/overview/test_fixture_record.sql",
        request_id,
        user_id,
        at,
        category,
        server_name,
        ok,
        state,
        100_i64,
        40_i64,
        10_i64,
        20_i64,
        120_i64,
        100_i64,
        30_i64,
    )
    .execute(pool)
    .await
    .expect("insert isolated request");
}

#[tokio::test]
async fn empty_short_windows_have_bounded_utc_buckets_and_absent_sample_values() {
    let pool = test_pg_target::required_pool(1)
        .await
        .expect("confirmed test database");
    let user = format!("empty-{}", uuid::Uuid::new_v4());
    let start = DateTime::<Utc>::from_timestamp(1_700_000_010, 0).unwrap();
    for (bucket, seconds, step) in [
        (OverviewBucket::Minute, 3_600, 60),
        (OverviewBucket::FiveMinute, 18_000, 300),
    ] {
        let rows = queries::query_trend(
            &pool,
            None,
            RequestRecordCategory::Ai,
            OverviewWindow {
                start: Some(start),
                end: Some(start + chrono::Duration::seconds(seconds)),
                bucket,
            },
            Some(&user),
            RequestRecordMcpScope::All,
        )
        .await
        .expect("empty trend");
        assert_eq!(rows.len(), 61);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(
                row.bucket_at.timestamp(),
                start.timestamp().div_euclid(step) * step + index as i64 * step
            );
            assert_eq!(row.request_count, 0);
            assert_eq!(row.success_rate, 0.0);
            assert_eq!(row.error_rate, 0.0);
            assert!(row.p95_total_ms.is_none());
            assert!(row.p95_first_token_ms.is_none());
            assert!(row.tokens.cache_rate.is_none());
            assert!(row.tokens.cache_hit_rate.is_none());
            assert_eq!(row.tokens.total_tokens, 0);
        }
    }
    pool.close().await;
}

#[tokio::test]
async fn real_events_obey_bounds_aggregates_visibility_scope_and_exact_cache() {
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;

    let pool = test_pg_target::required_pool(4)
        .await
        .expect("confirmed test database");
    let names = [
        format!("p3t-{}", uuid::Uuid::new_v4()),
        format!("p3t-{}", uuid::Uuid::new_v4()),
    ];
    let ids: Vec<_> = (0..8).map(|_| uuid::Uuid::new_v4()).collect();
    let mut users = Vec::new();
    let result = AssertUnwindSafe(async {
        for name in &names {
            let row = sqlx::query_file!("src/sql/usage/overview/test_fixture_user.sql", name)
                .fetch_one(&pool)
                .await
                .expect("insert fixture user");
            users.push(row.user_id);
        }
        let start = Utc::now()
            .date_naive()
            .and_hms_opt(12, 0, 10)
            .unwrap()
            .and_utc();
        for (i, offset, user, category, server, ok) in [
            (0, -1, users[0], "ai", None, true),
            (1, 0, users[0], "ai", None, true),
            (2, 5, users[0], "ai", None, false),
            (3, 3600, users[0], "ai", None, true),
            (4, 5, users[1], "ai", None, true),
            (5, 5, users[0], "mcp", Some("p3t-service"), true),
            (6, 5, users[0], "mcp", None, false),
            (7, 18000, users[0], "ai", None, true),
        ] {
            insert_fixture_record(
                &pool,
                ids[i],
                user,
                start + chrono::Duration::seconds(offset),
                category,
                server,
                ok,
            )
            .await;
        }
        let overview = |user, category, scope, window| {
            request_records_overview(
                &pool,
                Some(user),
                category,
                window,
                None,
                RequestRecordOverviewPerspective::Model,
                scope,
            )
        };
        let window = OverviewWindow {
            start: Some(start),
            end: Some(start + chrono::Duration::hours(1)),
            bucket: OverviewBucket::Minute,
        };
        let first = overview(
            users[0],
            RequestRecordCategory::Ai,
            RequestRecordMcpScope::All,
            window,
        )
        .await
        .expect("first overview");
        assert_eq!(first.trend.len(), 61);
        assert_eq!(first.summary.request_count, 2);
        assert_eq!(
            first.trend.iter().map(|row| row.request_count).sum::<i64>(),
            2
        );
        assert_eq!(first.trend[0].success_count, 1);
        assert_eq!(first.trend[0].error_count, 1);
        assert_eq!(first.trend[0].success_rate, 0.5);
        assert_eq!(first.trend[0].error_rate, 0.5);
        assert_eq!(first.trend[0].tokens.input_tokens, 100);
        assert_eq!(first.trend[0].tokens.cache_read_tokens, 80);
        assert_eq!(first.trend[0].tokens.cache_write_tokens, 20);
        assert_eq!(first.trend[0].tokens.total_tokens, 240);
        assert_eq!(first.trend[0].p95_total_ms, Some(100.0));
        assert_eq!(first.trend[0].p95_first_token_ms, Some(30.0));
        assert_eq!(first.trend[1].request_count, 0);
        let shifted = OverviewWindow {
            start: Some(start + chrono::Duration::seconds(10)),
            end: Some(start + chrono::Duration::hours(1) + chrono::Duration::seconds(10)),
            ..window
        };
        let second = overview(
            users[0],
            RequestRecordCategory::Ai,
            RequestRecordMcpScope::All,
            shifted,
        )
        .await
        .expect("shifted overview");
        assert_eq!(second.summary.request_count, 1);
        assert_eq!(
            second
                .trend
                .iter()
                .map(|row| row.request_count)
                .sum::<i64>(),
            1
        );
        let other = overview(
            users[1],
            RequestRecordCategory::Ai,
            RequestRecordMcpScope::All,
            window,
        )
        .await
        .expect("other user");
        assert_eq!(other.summary.request_count, 1);
        for (scope, errors) in [
            (RequestRecordMcpScope::Service, 0),
            (RequestRecordMcpScope::Gateway, 1),
        ] {
            let mcp = overview(users[0], RequestRecordCategory::Mcp, scope, window)
                .await
                .expect("mcp overview");
            assert_eq!(mcp.summary.request_count, 1);
            assert_eq!(
                mcp.trend.iter().map(|row| row.error_count).sum::<i64>(),
                errors
            );
        }
        let five = OverviewWindow {
            start: Some(start),
            end: Some(start + chrono::Duration::hours(5)),
            bucket: OverviewBucket::FiveMinute,
        };
        let five_rows = queries::query_trend(
            &pool,
            Some(users[0]),
            RequestRecordCategory::Ai,
            five,
            None,
            RequestRecordMcpScope::All,
        )
        .await
        .expect("five minute trend");
        assert_eq!(five_rows.len(), 61);
        assert_eq!(five_rows[0].request_count, 2);
        assert_eq!(five_rows[12].request_count, 1);
        assert_eq!(five_rows[60].request_count, 0);
        assert_eq!(
            five_rows.iter().map(|row| row.request_count).sum::<i64>(),
            3
        );
    })
    .catch_unwind()
    .await;
    let cleanup_records = sqlx::query_file!(
        "src/sql/usage/overview/test_fixture_cleanup_records.sql",
        &ids,
        &users
    )
    .execute(&pool)
    .await;
    let cleanup_users = sqlx::query_file!(
        "src/sql/usage/overview/test_fixture_cleanup_users.sql",
        &users,
        &names[..]
    )
    .execute(&pool)
    .await;
    pool.close().await;
    let removed_records = cleanup_records.unwrap_or_else(|error| {
        panic!("fixture cleanup records failed; request IDs {ids:?}, user IDs {users:?}: {error}")
    });
    let removed_users = cleanup_users.unwrap_or_else(|error| {
        panic!("fixture cleanup users failed; user IDs {users:?}, names {names:?}: {error}")
    });
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
    assert_eq!(removed_records.rows_affected(), 8);
    assert_eq!(removed_users.rows_affected(), 2);
}
