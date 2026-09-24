//! Issue #277 Phase P8 — full-history and scoped admin clear semantics.
//!
//! Only a full-scope, unbounded clear drops history partitions. Every
//! per-user clear is a bounded row DELETE across the whole request family, so
//! a non-admin clearing their own records can never destroy another user's
//! history. Windowed clears live in `partition_clear_windowed.rs`.

#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/partition_clear_support.rs"]
mod support;

use chrono::{Duration, Utc};
use prompt_ferry::db;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::support::{
    assert_family_rows_absent, clear_query, count_metadata, count_partition, create_record,
    create_today_record_with_family, create_user,
};

#[tokio::test]
async fn full_clear_drops_yesterday_and_erases_today_family() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let user = create_user(&schema.pool, "p8-full-clear").await?;

    // History row on yesterday's partition, plus a full family for today.
    let history_event = create_record(&schema.pool, Some(user)).await?;
    let yesterday = Utc::now() - Duration::days(1);
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        history_event,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_content_created_at.sql",
        history_event,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;

    let (today_event, _, block_hash) =
        create_today_record_with_family(&schema.pool, Some(user)).await?;

    let yesterday_partition = format!("request_records_{}", yesterday.format("%Y%m%d"));
    assert_eq!(
        count_partition(&schema.pool, &yesterday_partition).await?,
        1,
        "yesterday's partition exists before the clear"
    );

    let report = db::clear_usage_events(
        &schema.pool,
        clear_query(db::UsageClearScope::AllUsers, None, None, None, None),
    )
    .await?;
    assert!(
        report.deleted >= 1,
        "a full clear reports dropped partitions plus today's rows"
    );
    assert_eq!(report.protected_by_billing, 0);

    // Yesterday's whole-day partition is gone: a full clear is not limited to
    // the retention horizon that keeps the previous day.
    assert_eq!(
        count_partition(&schema.pool, &yesterday_partition).await?,
        0,
        "a full clear drops yesterday's partition"
    );
    assert_eq!(
        count_metadata(&schema.pool, history_event).await?,
        0,
        "history metadata leaves with its partition"
    );
    assert_eq!(
        count_metadata(&schema.pool, today_event).await?,
        0,
        "today's metadata row is deleted"
    );
    assert_family_rows_absent(&schema.pool, today_event, &block_hash).await?;

    // The current day stays writable.
    let after = create_record(&schema.pool, Some(user)).await?;
    assert_eq!(
        count_metadata(&schema.pool, after).await?,
        1,
        "today accepts writes after a full clear"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn scoped_clear_deletes_only_that_user_and_keeps_global_history() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let user_a = create_user(&schema.pool, "p8-scoped-a").await?;
    let user_b = create_user(&schema.pool, "p8-scoped-b").await?;

    let b_history = create_record(&schema.pool, Some(user_b)).await?;
    let yesterday = Utc::now() - Duration::days(1);
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        b_history,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_content_created_at.sql",
        b_history,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;
    let b_today = create_record(&schema.pool, Some(user_b)).await?;
    let a_today = create_record(&schema.pool, Some(user_a)).await?;

    let report = db::clear_usage_events(
        &schema.pool,
        clear_query(
            db::UsageClearScope::CurrentUser,
            Some(user_a),
            None,
            None,
            None,
        ),
    )
    .await?;
    assert_eq!(
        report.deleted, 1,
        "a scoped clear deletes only the caller's rows"
    );

    let yesterday_partition = format!("request_records_{}", yesterday.format("%Y%m%d"));
    assert_eq!(
        count_partition(&schema.pool, &yesterday_partition).await?,
        1,
        "a per-user clear never drops another user's history partition"
    );
    for (label, event_id, expected) in [
        ("other user's history", b_history, 1),
        ("other user's today", b_today, 1),
        ("caller's today", a_today, 0),
    ] {
        let count = count_metadata(&schema.pool, event_id).await?;
        assert_eq!(count, expected, "{label} unexpected after clear");
    }

    schema.cleanup().await?;
    Ok(())
}
