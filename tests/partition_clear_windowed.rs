//! Issue #277 Phase P8 — windowed admin clear is a bounded row DELETE.

#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/partition_clear_support.rs"]
mod support;

use chrono::{Duration, Utc};
use prompt_ferry::db;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::support::{clear_query, count_metadata, count_partition, create_record, create_user};

#[tokio::test]
async fn windowed_clear_deletes_only_rows_inside_the_window() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let user = create_user(&schema.pool, "p8-windowed").await?;

    let old_event = create_record(&schema.pool, Some(user)).await?;
    let yesterday = Utc::now() - Duration::days(1);
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        old_event,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_content_created_at.sql",
        old_event,
        yesterday,
    )
    .execute(&schema.pool)
    .await?;
    let recent_event = create_record(&schema.pool, Some(user)).await?;

    let today_midnight = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc();
    let report = db::clear_usage_events(
        &schema.pool,
        clear_query(
            db::UsageClearScope::AllUsers,
            None,
            None,
            Some(today_midnight),
            None,
        ),
    )
    .await?;
    assert_eq!(report.deleted, 1, "only today's row is inside the window");

    let yesterday_partition = format!("request_records_{}", yesterday.format("%Y%m%d"));
    assert_eq!(
        count_partition(&schema.pool, &yesterday_partition).await?,
        1,
        "a windowed clear never drops a partition"
    );
    for (label, event_id, expected) in [("old row", old_event, 1), ("recent row", recent_event, 0)]
    {
        let count = count_metadata(&schema.pool, event_id).await?;
        assert_eq!(count, expected, "{label} unexpected after clear");
    }

    schema.cleanup().await?;
    Ok(())
}
