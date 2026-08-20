use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::db::{RequestRecordCategory, RequestRecordOverviewResponse};

mod presentation;
mod queries;

pub use self::queries::OverviewBucket;

#[derive(Debug, Clone, Copy)]
pub struct OverviewWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub bucket: OverviewBucket,
}

pub async fn request_records_overview(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<RequestRecordOverviewResponse> {
    let (summary, trend, breakdown, error_breakdown) = tokio::try_join!(
        queries::query_summary(pool, visible_user_id, request_category, window, user),
        queries::query_trend(pool, visible_user_id, request_category, window, user),
        queries::query_breakdown(pool, visible_user_id, request_category, window, user),
        queries::query_error_breakdown(pool, visible_user_id, request_category, window, user),
    )?;

    Ok(RequestRecordOverviewResponse {
        summary,
        trend,
        breakdown,
        error_breakdown,
    })
}
