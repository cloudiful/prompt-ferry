use anyhow::Result;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// AI tokens recorded for one endpoint since the start of the UTC day.
///
/// Feeds the admin token-plan badge for balance providers that carry no
/// provider-side spend figure (DeepSeek): the badge pairs the account
/// balance with this locally aggregated "today usage" number.
pub async fn endpoint_today_tokens(
    pool: &PgPool,
    endpoint_id: Uuid,
    now: DateTime<Utc>,
) -> Result<i64> {
    let day_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .expect("valid UTC day start");
    let row = sqlx::query_file!(
        "src/sql/usage/endpoint_today_tokens.sql",
        endpoint_id,
        day_start,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.total_tokens)
}
