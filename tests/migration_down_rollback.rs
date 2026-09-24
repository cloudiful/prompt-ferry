//! Issue #277 Phase P10 — down-migration executability with the FK-carrying
//! side tables populated.
//!
//! The partition-family down migration rebuilds `request_records` as an empty
//! legacy table. Every surviving table that carries a foreign key into it
//! would fail the restored constraint, so the down path truncates all of them
//! first — data loss in both directions is operator-approved — and the
//! original FK shape must come back without drift.

#[path = "support/db_harness.rs"]
mod db_harness;

use prompt_ferry::db;
use sqlx::Executor;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

async fn create_user(pool: &sqlx::PgPool) -> anyhow::Result<i64> {
    let user = db::create_user(
        pool,
        db::UserCreate {
            login_name: format!("down_round_trip_{}", Uuid::new_v4().simple()),
            password_hash: prompt_ferry::keys::hash_password("password-123")?,
            display_name: "down round trip".to_string(),
            is_admin: false,
        },
    )
    .await?;
    Ok(user.user_id)
}

/// Raw payload metadata exactly as the shipped write path persists it.
async fn seed_raw_payload(
    pool: &sqlx::PgPool,
    event_id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<()> {
    sqlx::query_file!(
        "tests/sql/db_migrations/seed_raw_payload_object.sql",
        event_id,
        created_at,
        format!("prompt-ferry/raw/events/{event_id}.bin"),
        16_i64,
        "ab".repeat(32),
        created_at + chrono::Duration::days(3),
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn down_migrates_while_billing_ledger_holds_rows() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    db::migrate(&schema.pool).await?;

    let user_id = create_user(&schema.pool).await?;
    let now = chrono::Utc::now();

    // Billing ledger: two charges (one bare INSERT, one produced by the real
    // write path — an AI event carrying usage numbers is charged
    // automatically by `record_usage_charge`, which adds 4 meter lines)
    // plus a redaction session, so every ledger truncation target carries
    // FK-carrying rows.
    sqlx::query_file!(
        "tests/sql/db_migrations/seed_usage_charge.sql",
        910_001_i64,
        user_id,
        Uuid::new_v4()
    )
    .execute(&schema.pool)
    .await?;
    let mut charged_event = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    charged_event.input_tokens = Some(10);
    charged_event.output_tokens = Some(5);
    charged_event.total_tokens = Some(15);
    db::record_request_record(&schema.pool, charged_event).await?;
    db::upsert_conversation_redaction_session(
        &schema.pool,
        db::ConversationRedactionSessionCreate {
            conversation_id: Uuid::new_v4(),
            session_ciphertext: vec![1, 2, 3],
            session_nonce: vec![4, 5, 6],
            session_key_version: 1,
            last_event_id: Some(910_001),
            policy_version: 1,
        },
    )
    .await?;

    // Raw payloads, the reviewer-reproduced failure shape. The daily
    // partition is created first (an empty `_default`), then the rows land:
    // an orphan in `_default` (outside every daily partition's bounds) and
    // rows referencing real events inside the daily partition.
    let partition_day = now.date_naive();
    let next_day = partition_day + chrono::Duration::days(1);
    let start = partition_day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = next_day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    schema
        .pool
        .execute(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE IF NOT EXISTS request_record_raw_payloads_p{} \
             PARTITION OF request_record_raw_payloads \
             FOR VALUES FROM ('{}') TO ('{}')",
            partition_day.format("%Y%m%d"),
            start.to_rfc3339(),
            end.to_rfc3339(),
        )))
        .await?;
    seed_raw_payload(&schema.pool, 910_002, now - chrono::Duration::days(10)).await?;
    let real_event = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    let real_event_id = db::record_request_record(&schema.pool, real_event).await?;
    seed_raw_payload(&schema.pool, real_event_id, now).await?;
    seed_raw_payload(&schema.pool, 910_003, start + chrono::Duration::hours(1)).await?;
    sqlx::query_file!(
        "tests/sql/db_migrations/seed_raw_payload_overflow.sql",
        real_event_id,
        now,
        format!("prompt-ferry/raw/events/{real_event_id}.overflow.bin"),
        16_i64,
        "cd".repeat(32),
        now + chrono::Duration::days(3),
    )
    .execute(&schema.pool)
    .await?;

    let seeded = sqlx::query_file!("tests/sql/db_migrations/ledger_round_trip_counts.sql")
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(seeded.charges, 3);
    assert_eq!(seeded.charge_lines, 4);
    assert_eq!(seeded.raw_payloads, 3);
    assert_eq!(seeded.raw_payload_overflow, 1);

    // Down must succeed despite the non-empty ledger and raw payloads ...
    db::revert_latest_migration(&schema.pool).await?;

    // ... truncating every FK-carrying table ...
    let drained = sqlx::query_file!("tests/sql/db_migrations/ledger_round_trip_counts.sql")
        .fetch_one(&schema.pool)
        .await?;
    assert_eq!(drained.charges, 0);
    assert_eq!(drained.charge_lines, 0);
    assert_eq!(drained.redaction_sessions, 0);
    assert_eq!(drained.raw_payloads, 0);
    assert_eq!(drained.raw_payload_overflow, 0);

    // ... and restoring the pre-partition FK shape with no drift.
    let shape = sqlx::query_file!("tests/sql/db_migrations/legacy_family_fk_shape.sql")
        .fetch_one(&schema.pool)
        .await?;
    assert!(shape.charges_fkey);
    assert!(shape.redaction_fkey);
    assert!(shape.raw_payloads_fkey);
    assert!(shape.raw_payloads_overflow_fkey);
    assert!(shape.records_plain);

    // Up again on top of the downed schema.
    db::migrate(&schema.pool).await?;

    schema.cleanup().await?;
    Ok(())
}
