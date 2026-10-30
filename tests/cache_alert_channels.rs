use anyhow::{Result, ensure};
use prompt_ferry::{db, notify};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// The migration keeps every pre-existing row on the DingTalk channel and adds
/// an independent SMTP cooldown row per conversation. This test uses only its
/// own random conversation ids, reads the dedicated confirmed test target, and
/// deletes exactly those rows again.
#[tokio::test]
async fn legacy_dingtalk_rows_and_smtp_cooldowns_stay_independent() {
    let pool = test_pg_target::required_pool(2)
        .await
        .expect("connect to the confirmed quota test database");
    let legacy_id = Uuid::new_v4();
    let smtp_only_id = Uuid::new_v4();
    let ids = vec![legacy_id, smtp_only_id];

    let outcome = exercise(&pool, legacy_id, smtp_only_id).await;

    let cleanup = sqlx::query("DELETE FROM cache_alert_state WHERE conversation_id = ANY($1)")
        .bind(&ids)
        .execute(&pool)
        .await;
    outcome.expect("per-channel cooldown rows behave as designed");
    cleanup.expect("delete only this test's random conversation rows");
}

async fn exercise(pool: &PgPool, legacy_id: Uuid, smtp_only_id: Uuid) -> Result<()> {
    // The legacy single-channel insert path: no `channel` column, so the
    // migration default has to make this an ordinary DingTalk row.
    sqlx::query(
        "INSERT INTO cache_alert_state \
         (conversation_id, last_alerted_at, last_cache_rate, last_turns) \
         VALUES ($1, NOW() - INTERVAL '2 hours', 0.1, 3)",
    )
    .bind(legacy_id)
    .execute(pool)
    .await?;

    let channel: String =
        sqlx::query_scalar("SELECT channel FROM cache_alert_state WHERE conversation_id = $1")
            .bind(legacy_id)
            .fetch_one(pool)
            .await?;
    ensure!(
        channel == notify::CACHE_ALERT_CHANNEL_DINGTALK,
        "a legacy row must keep the dingtalk channel, got {channel}"
    );

    let dingtalk_before =
        db::last_cache_alert_at(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_DINGTALK)
            .await?
            .expect("the legacy row supplies the DingTalk cooldown");
    ensure!(
        db::last_cache_alert_at(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_SMTP)
            .await?
            .is_none(),
        "a new SMTP channel must start without a cooldown"
    );

    db::record_cache_alert(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_SMTP, 0.25, 9).await?;

    let dingtalk_after =
        db::last_cache_alert_at(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_DINGTALK)
            .await?
            .expect("the DingTalk row still exists");
    ensure!(
        dingtalk_after == dingtalk_before,
        "recording an SMTP alert must not move the DingTalk cooldown"
    );
    let smtp_after = db::last_cache_alert_at(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_SMTP)
        .await?
        .expect("the SMTP cooldown was recorded");
    ensure!(
        smtp_after > dingtalk_before,
        "the SMTP cooldown is a fresh database-clock timestamp"
    );

    let rows: Vec<(String, f64, i32)> = sqlx::query(
        "SELECT channel, last_cache_rate, last_turns \
         FROM cache_alert_state WHERE conversation_id = $1 ORDER BY channel",
    )
    .bind(legacy_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        (
            row.get::<String, _>("channel"),
            row.get::<f64, _>("last_cache_rate"),
            row.get::<i32, _>("last_turns"),
        )
    })
    .collect();
    ensure!(
        rows == vec![
            ("dingtalk".to_string(), 0.1, 3),
            ("smtp".to_string(), 0.25, 9)
        ],
        "each channel keeps its own row, rate, and turn count: {rows:?}"
    );

    // Repeating the same channel updates its own row instead of adding one.
    let smtp_before_repeat = smtp_after;
    db::record_cache_alert(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_SMTP, 0.3, 10).await?;
    let row_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM cache_alert_state WHERE conversation_id = $1")
            .bind(legacy_id)
            .fetch_one(pool)
            .await?;
    ensure!(
        row_count == 2,
        "the composite key upserts instead of appending"
    );
    let smtp_repeated = db::last_cache_alert_at(pool, legacy_id, notify::CACHE_ALERT_CHANNEL_SMTP)
        .await?
        .expect("the SMTP cooldown still exists");
    ensure!(
        smtp_repeated >= smtp_before_repeat,
        "an SMTP retry advances or keeps its own cooldown"
    );

    // A conversation alerted only through SMTP has no DingTalk cooldown, so a
    // later DingTalk configuration can deliver on its first pass.
    db::record_cache_alert(
        pool,
        smtp_only_id,
        notify::CACHE_ALERT_CHANNEL_SMTP,
        0.15,
        6,
    )
    .await?;
    ensure!(
        db::last_cache_alert_at(pool, smtp_only_id, notify::CACHE_ALERT_CHANNEL_SMTP)
            .await?
            .is_some(),
        "the SMTP-only conversation recorded its cooldown"
    );
    ensure!(
        db::last_cache_alert_at(pool, smtp_only_id, notify::CACHE_ALERT_CHANNEL_DINGTALK)
            .await?
            .is_none(),
        "an SMTP-only conversation has no DingTalk cooldown"
    );

    Ok(())
}
