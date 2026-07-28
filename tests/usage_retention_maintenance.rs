#[path = "support/db_harness.rs"]
mod db_harness;

use chrono::{Duration, Utc};
use prompt_ferry::db;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_record(
    pool: &sqlx::PgPool,
    request_id: Uuid,
    user_id: Option<i64>,
    state: db::RequestRecordState,
) -> anyhow::Result<i64> {
    Ok(db::record_request_record(
        pool,
        db::RequestRecordCreate::ai_request(request_id, "/v1/responses")
            .with_state(db::UsageEventKind::Request, state)
            .with_request_actor(user_id, None, None, None),
    )
    .await?)
}

async fn mark_record_old(pool: &sqlx::PgPool, event_id: i64) -> anyhow::Result<()> {
    sqlx::query_file!(
        "tests/sql/usage_maintenance/set_request_record_created_at.sql",
        event_id,
        Utc::now() - Duration::days(30),
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn metadata_prune_protects_billing_active_and_leased_records() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let unbilled_request_id = Uuid::new_v4();
    let unbilled_event = create_record(
        &schema.pool,
        unbilled_request_id,
        None,
        db::RequestRecordState::Completed,
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/delete_usage_charge.sql",
        unbilled_event,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_request_record_lease.sql",
        unbilled_request_id,
        Utc::now() - Duration::hours(1),
        Utc::now() - Duration::hours(2),
    )
    .execute(&schema.pool)
    .await?;
    mark_record_old(&schema.pool, unbilled_event).await?;

    let billed_event = create_record(
        &schema.pool,
        Uuid::new_v4(),
        None,
        db::RequestRecordState::Completed,
    )
    .await?;
    mark_record_old(&schema.pool, billed_event).await?;

    let active_request_id = Uuid::new_v4();
    let active_event = create_record(
        &schema.pool,
        active_request_id,
        None,
        db::RequestRecordState::Received,
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/delete_usage_charge.sql",
        active_event,
    )
    .execute(&schema.pool)
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/insert_request_record_lease.sql",
        active_request_id,
        Utc::now() + Duration::hours(1),
        Utc::now(),
    )
    .execute(&schema.pool)
    .await?;
    mark_record_old(&schema.pool, active_event).await?;

    let report = db::prune_usage_events(&schema.pool, 1).await?;
    assert_eq!(report.deleted, 1);
    assert_eq!(report.protected_by_billing, 1);

    let deleted = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        unbilled_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(deleted.count, 0);
    let billed = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        billed_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(billed.count, 1);
    let active = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        active_event,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(active.count, 1);
    let orphan_lease = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record_leases.sql",
        unbilled_request_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(orphan_lease.count, 0);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn metadata_maintenance_skips_when_lock_is_held() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let lock_key = 0x7072_756e_654d_6574_i64;
    let mut lock_connection = schema.pool.acquire().await?;
    let acquired = sqlx::query_file_scalar!(
        "tests/sql/usage_maintenance/try_acquire_metadata_prune_lock.sql",
        lock_key,
    )
    .fetch_one(&mut *lock_connection)
    .await?;
    assert!(acquired);

    let skipped = db::run_usage_metadata_maintenance(&schema.pool, 1).await?;
    assert!(skipped.is_none());

    let released = sqlx::query_file_scalar!(
        "tests/sql/usage_maintenance/release_metadata_prune_lock.sql",
        lock_key,
    )
    .fetch_one(&mut *lock_connection)
    .await?;
    assert!(released);
    drop(lock_connection);

    assert!(
        db::run_usage_metadata_maintenance(&schema.pool, 1)
            .await?
            .is_some()
    );
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn approval_retention_keeps_pending_and_recent_resolved_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let create = |status: &str| db::ApprovalRequestCreate {
        approval_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        user_id: None,
        client_key_label: None,
        path: "/v1/responses".to_string(),
        model: Some("test-model".to_string()),
        review_decision: "flag".to_string(),
        approval_status: status.to_string(),
        review_reason: "test".to_string(),
        review_categories: Vec::new(),
        request_preview: "test".to_string(),
        request_payload_json: None,
        request_deadline_unix_ms: 0,
        wait_deadline_unix_ms: 0,
    };
    let pending = db::create_approval_request(&schema.pool, create("pending")).await?;
    let expired = db::create_approval_request(&schema.pool, create("approved")).await?;
    let recent = db::create_approval_request(&schema.pool, create("rejected")).await?;
    let old = Utc::now() - Duration::days(30);
    for approval_id in [pending.approval_id, expired.approval_id] {
        sqlx::query_file!(
            "tests/sql/usage_maintenance/set_approval_created_at.sql",
            approval_id,
            old,
        )
        .execute(&schema.pool)
        .await?;
    }

    let first = db::run_approval_retention_maintenance(&schema.pool, 1)
        .await?
        .expect("approval retention should acquire its advisory lock");
    let second = db::run_approval_retention_maintenance(&schema.pool, 1)
        .await?
        .expect("approval retention should release its advisory lock");
    assert_eq!(first + second, 1);

    for (approval_id, expected) in [
        (pending.approval_id, 1),
        (expired.approval_id, 0),
        (recent.approval_id, 1),
    ] {
        let count = sqlx::query_file!(
            "tests/sql/usage_maintenance/count_approval.sql",
            approval_id,
        )
        .fetch_one(&schema.pool)
        .await?;
        assert_eq!(count.count, expected);
    }

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn clear_scopes_keep_billed_records_and_report_protection() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;
    let user_a = db::create_user(
        &schema.pool,
        db::UserCreate {
            login_name: "retention-user-a".to_string(),
            password_hash: "unused".to_string(),
            display_name: "Retention A".to_string(),
            is_admin: false,
        },
    )
    .await?
    .user_id;
    let user_b = db::create_user(
        &schema.pool,
        db::UserCreate {
            login_name: "retention-user-b".to_string(),
            password_hash: "unused".to_string(),
            display_name: "Retention B".to_string(),
            is_admin: false,
        },
    )
    .await?
    .user_id;

    let current_unbilled = create_record(
        &schema.pool,
        Uuid::new_v4(),
        Some(user_a),
        db::RequestRecordState::Completed,
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/delete_usage_charge.sql",
        current_unbilled,
    )
    .execute(&schema.pool)
    .await?;
    let current_billed = create_record(
        &schema.pool,
        Uuid::new_v4(),
        Some(user_a),
        db::RequestRecordState::Completed,
    )
    .await?;
    let current = db::clear_usage_events(
        &schema.pool,
        db::UsageClearQuery {
            scope: db::UsageClearScope::CurrentUser,
            visible_user_id: Some(user_a),
            target_user_id: None,
            start_at: None,
            end_at: None,
        },
    )
    .await?;
    assert_eq!(current.deleted, 1);
    assert_eq!(current.protected_by_billing, 1);

    let target = create_record(
        &schema.pool,
        Uuid::new_v4(),
        Some(user_b),
        db::RequestRecordState::Completed,
    )
    .await?;
    sqlx::query_file!(
        "tests/sql/usage_maintenance/delete_usage_charge.sql",
        target,
    )
    .execute(&schema.pool)
    .await?;
    let target_report = db::clear_usage_events(
        &schema.pool,
        db::UsageClearQuery {
            scope: db::UsageClearScope::TargetUser,
            visible_user_id: None,
            target_user_id: Some(user_b),
            start_at: None,
            end_at: None,
        },
    )
    .await?;
    assert_eq!(target_report.deleted, 1);

    let all_a = create_record(
        &schema.pool,
        Uuid::new_v4(),
        Some(user_a),
        db::RequestRecordState::Completed,
    )
    .await?;
    let all_b = create_record(
        &schema.pool,
        Uuid::new_v4(),
        Some(user_b),
        db::RequestRecordState::Completed,
    )
    .await?;
    for event_id in [all_a, all_b] {
        sqlx::query_file!(
            "tests/sql/usage_maintenance/delete_usage_charge.sql",
            event_id,
        )
        .execute(&schema.pool)
        .await?;
    }
    let all = db::clear_usage_events(
        &schema.pool,
        db::UsageClearQuery {
            scope: db::UsageClearScope::AllUsers,
            visible_user_id: None,
            target_user_id: None,
            start_at: None,
            end_at: None,
        },
    )
    .await?;
    assert_eq!(all.deleted, 2);
    assert_eq!(all.protected_by_billing, 1);

    let billed_count = sqlx::query_file!(
        "tests/sql/usage_maintenance/count_request_record.sql",
        current_billed,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(billed_count.count, 1);

    schema.cleanup().await?;
    Ok(())
}
