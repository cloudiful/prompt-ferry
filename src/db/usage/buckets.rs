use super::*;
use crate::db::RequestRecordBucket;

pub async fn usage_buckets(
    pool: &PgPool,
    bucket: &str,
    limit: i64,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    visible_user_id: Option<i64>,
) -> Result<Vec<RequestRecordBucket>> {
    let limit = limit.saturating_sub(1);
    match bucket {
        "minute" => Ok(sqlx::query_file_as!(
            RequestRecordBucket,
            "src/sql/usage/usage_buckets_minute.sql",
            limit,
            start,
            end,
            visible_user_id,
        )
        .fetch_all(pool)
        .await?),
        "day" => Ok(sqlx::query_file_as!(
            RequestRecordBucket,
            "src/sql/usage/usage_buckets_day.sql",
            limit,
            start,
            end,
            visible_user_id,
        )
        .fetch_all(pool)
        .await?),
        _ => Ok(sqlx::query_file_as!(
            RequestRecordBucket,
            "src/sql/usage/usage_buckets_hour.sql",
            limit,
            start,
            end,
            visible_user_id,
        )
        .fetch_all(pool)
        .await?),
    }
}
