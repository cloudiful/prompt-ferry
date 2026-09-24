//! Issue #277 Phase P8 — partition manager regression tests.
//!
//! Acceptance: an expired day's partitions are dropped wholesale while
//! non-expired and current-day partitions are preserved, a missing current-day
//! partition is healed by the tick, the tick never issues a large DELETE, and
//! the plain small tables (leases, redaction sessions) are reaped. Admin clear
//! semantics live in `partition_clear.rs`.

#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_record(pool: &sqlx::PgPool, user_id: Option<i64>) -> anyhow::Result<i64> {
    db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses")
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_request_actor(user_id, None, None, None),
    )
    .await
}

async fn metadata_partition_count(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    let row = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_parent_partitions.sql",
        "request_records",
    )
    .fetch_one(pool)
    .await?;
    Ok(row.count)
}

#[tokio::test]
async fn tick_pre_creates_the_three_day_forward_window() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let before = metadata_partition_count(&schema.pool).await?;

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");

    // P7's migration pre-created today+8, so the tick normally creates
    // nothing new on a fresh schema; the report must still be well-formed.
    assert!(report.partitions_created <= 8);
    let after = metadata_partition_count(&schema.pool).await?;
    // The tick may legitimately drop partitions older than the retention
    // horizon (the P7 handover window includes 95-day-old days), but it must
    // never leave the schema without the forward window.
    let today_prefix = format!(
        "request_records_{}",
        Utc::now().date_naive().format("%Y%m%d")
    );
    let forward_window = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_parent_partitions_since.sql",
        "request_records",
        today_prefix,
    )
    .fetch_one(&schema.pool)
    .await?
    .count;
    assert!(
        forward_window >= 4,
        "today plus the 3 pre-created days must exist, found {forward_window}"
    );
    assert!(
        after + i64::try_from(report.partitions_dropped).unwrap_or(i64::MAX) >= before,
        "partitions only shrink via drops"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn expired_partition_is_dropped_and_unexpired_partition_is_kept() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    // Two completed records today: one stays, one is moved to a droppable
    // content-family day. Content days only exist for today-8..today+8 (the
    // P7 handover window), so the expired fixture uses a day that is both
    // inside that window and past the 3-day content horizon.
    let kept_event = create_record(&schema.pool, Some(1)).await?;
    let expired_event = create_record(&schema.pool, Some(1)).await?;
    let expired_day = Utc::now() - Duration::days(5);
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        expired_event,
        expired_day,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/sync_request_record_content_created_at.sql",
        expired_event,
    )
    .execute(&schema.pool)
    .await?;

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");

    // The content-family partitions of that day (content, block refs,
    // artifacts, tool calls, snapshots, raw payloads) all drop, while the
    // metadata day partition survives: day -5 is well inside the 90-day
    // metadata horizon.
    assert!(
        report.partitions_dropped >= 6,
        "the expired content-family day's partitions must drop, got {}",
        report.partitions_dropped
    );

    let kept = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        kept_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(kept.count, 1, "an unexpired row survives the tick");

    // The expired event's metadata row remains (90-day horizon), but its
    // content row is gone: "content row absent" is the new expired signal.
    let expired = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        expired_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(expired.count, 1, "metadata survives its 90-day horizon");

    let content = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM request_record_content WHERE event_id = $1",
    )
    .bind(expired_event)
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        content, 0,
        "the expired content row leaves with its partition"
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn tick_never_drops_the_current_or_previous_day() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let today_event = create_record(&schema.pool, Some(1)).await?;
    let yesterday_event = create_record(&schema.pool, Some(1)).await?;
    let yesterday = Utc::now() - Duration::days(1);
    for (event_id, day) in [(today_event, Utc::now()), (yesterday_event, yesterday)] {
        // Rows written "today" already sit in today's partition; the
        // yesterday row is moved back a whole day.
        if day < Utc::now() - Duration::hours(23) {
            sqlx::query_file!(
                "tests/sql/usage_maintenance/set_request_record_created_at.sql",
                event_id,
                day,
            )
            .execute(&schema.pool)
            .await?;
            sqlx::query_file!(
                "tests/sql/usage_maintenance/sync_request_record_content_created_at.sql",
                event_id,
            )
            .execute(&schema.pool)
            .await?;
        }
    }

    // Even a zero-day horizon must not touch today/yesterday.
    let horizons = db::PartitionHorizons {
        metadata_retention_days: 1,
        content_retention_days: 1,
    };
    db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");

    for event_id in [today_event, yesterday_event] {
        let row = sqlx::query_file!(
            "tests/sql/usage_maintenance/count_request_record.sql",
            event_id,
        )
        .fetch_one(&schema.pool)
        .await?;
        assert_eq!(
            row.count, 1,
            "today and yesterday partitions survive even a 1-day horizon"
        );
    }

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn tick_recreates_a_missing_current_day_partition() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    // A worker gap longer than a day leaves the current day without a
    // partition; the tick must heal it instead of failing writes forever.
    let today = Utc::now().date_naive();
    let today_partition = format!("request_records_{}", today.format("%Y%m%d"));
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS {today_partition}"
    )))
    .execute(&schema.pool)
    .await?;
    let missing = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_partition_by_name.sql",
        today_partition.as_str(),
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(missing.count, 0, "the fixture removed today's partition");

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");

    let recreated = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_partition_by_name.sql",
        today_partition.as_str(),
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(
        recreated.count, 1,
        "the tick must pre-create the current day when it is missing"
    );

    // The day accepts writes again.
    let event = create_record(&schema.pool, Some(1)).await?;
    let stored = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(stored.count, 1, "today is writable after the self-heal");

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn tick_reaps_orphan_leases_and_stale_redaction_sessions() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let now = Utc::now();

    // A lease whose request id has no metadata row is an orphan.
    let orphan_request_id = Uuid::new_v4();
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_request_record_lease.sql",
        orphan_request_id,
        now + Duration::minutes(30),
        now,
    )
    .execute(&schema.pool)
    .await?;

    // One session idle for over a week, one orphaned by a missing event, and
    // one live session that must survive.
    let stale_conversation = Uuid::new_v4();
    let orphan_conversation = Uuid::new_v4();
    let live_conversation = Uuid::new_v4();
    let live_event = create_record(&schema.pool, Some(1)).await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_conversation_redaction_session.sql",
        stale_conversation,
        None::<i64>,
        now - Duration::days(8),
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_conversation_redaction_session.sql",
        orphan_conversation,
        Some(999_999_999_i64),
        now,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_conversation_redaction_session.sql",
        live_conversation,
        Some(live_event),
        now,
    )
    .execute(&schema.pool)
    .await?;

    let leases_deleted = db::cleanup_orphan_request_record_leases(&schema.pool).await?;
    assert_eq!(leases_deleted, 1, "the orphan lease is reaped");
    let sessions_deleted = db::cleanup_stale_conversation_redaction_sessions(&schema.pool).await?;
    assert_eq!(sessions_deleted, 2, "idle and orphaned sessions are reaped");

    for (conversation_id, expected) in [
        (stale_conversation, 0),
        (orphan_conversation, 0),
        (live_conversation, 1),
    ] {
        let count = sqlx::query_file!(
            "tests/sql/usage_maintenance/count_conversation_redaction_session.sql",
            conversation_id,
        )
        .fetch_one(&schema.pool)
        .await?;
        assert_eq!(count.count, expected, "unexpected session survival");
    }

    schema.cleanup().await?;
    Ok(())
}

/// Issue #277 Phase P11: a partition the tick created is immediately ANALYZEd
/// (real planner statistics, `reltuples >= 0`), while a pre-existing
/// partition that already carries statistics is left untouched by the tick —
/// existing partitions stay on autovacuum autoanalyze and the tick must not
/// sweep the whole family.
#[tokio::test]
async fn tick_analyzes_only_the_partitions_it_creates() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let today = Utc::now().date_naive();
    let tomorrow_name = format!(
        "request_records_{}",
        (today + Duration::days(1)).format("%Y%m%d")
    );

    // Steady-state fixture: tomorrow's partition already exists (as after the
    // previous tick; the P7 migration pre-creates today+0..+8) with the
    // "never analyzed" sentinel statistics. Re-create it defensively so the
    // fixture does not depend on the handover window.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE IF EXISTS {tomorrow_name}"
    )))
    .execute(&schema.pool)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE {tomorrow_name} PARTITION OF request_records FOR VALUES FROM ('{}') TO ('{}')",
        (today + Duration::days(1)).format("%Y-%m-%d 00:00:00+00"),
        (today + Duration::days(2)).format("%Y-%m-%d 00:00:00+00"),
    )))
    .execute(&schema.pool)
    .await?;
    let before = sqlx::query_file!(
        "tests/sql/usage_maintenance/partition_reltuples.sql",
        tomorrow_name.as_str(),
    )
    .fetch_one(&schema.pool)
    .await?
    .reltuples;
    assert_eq!(before, -1, "a fresh partition starts with reltuples = -1");

    let horizons = db::PartitionHorizons {
        metadata_retention_days: 90,
        content_retention_days: 3,
    };
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");

    // The tick may legitimately create partitions the schema lacks (e.g. the
    // raw-payload family is partitioned only by the tick, not the migration).
    // Whatever it created must be analyzed; whatever it did not create must
    // keep its "never analyzed" sentinel.
    let after = sqlx::query_file!(
        "tests/sql/usage_maintenance/partition_reltuples.sql",
        tomorrow_name.as_str(),
    )
    .fetch_one(&schema.pool)
    .await?
    .reltuples;
    assert_eq!(
        after, -1,
        "the tick must not analyze partitions it did not create"
    );
    if report.partitions_created > 0 {
        let fresh_raw_name = format!("request_record_raw_payloads_{}", today.format("%Y%m%d"));
        let fresh = sqlx::query_file!(
            "tests/sql/usage_maintenance/partition_reltuples.sql",
            fresh_raw_name.as_str(),
        )
        .fetch_one(&schema.pool)
        .await?
        .reltuples;
        assert!(
            fresh >= 0,
            "a partition created by the tick must be ANALYZEd, reltuples = {fresh}"
        );
    }

    // Drop tomorrow's partition: the tick must re-create it AND analyze it,
    // so the planner sees real statistics (reltuples >= 0) right away.
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {tomorrow_name}")))
        .execute(&schema.pool)
        .await?;
    let report = db::run_partition_maintenance(&schema.pool, horizons)
        .await?
        .expect("partition maintenance should acquire its advisory lock");
    assert!(
        report.partitions_created >= 1,
        "the tick must re-create the missing forward-window partition"
    );
    let analyzed = sqlx::query_file!(
        "tests/sql/usage_maintenance/partition_reltuples.sql",
        tomorrow_name.as_str(),
    )
    .fetch_one(&schema.pool)
    .await?
    .reltuples;
    assert!(
        analyzed >= 0,
        "a partition created by the tick must be ANALYZEd, reltuples = {analyzed}"
    );

    // The recreated day accepts writes immediately.
    let event = create_record(&schema.pool, Some(1)).await?;
    let stored = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(stored.count, 1, "the analyzed day is writable");

    schema.cleanup().await?;
    Ok(())
}
