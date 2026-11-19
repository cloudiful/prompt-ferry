use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::db::{
    RequestRecordCategory, RequestRecordMcpScope, RequestRecordOverviewPerspective,
    RequestRecordOverviewResponse,
};

mod presentation;
mod queries;

/// Cache-rate presentation shared with the continuous-session cache alert
/// monitor so both surfaces agree on the fold-aware `cache_read / full_input`
/// semantics and the `[0, 1]` clamp.
pub(crate) use self::presentation::overview_cache_rate;
pub use self::queries::OverviewBucket;

#[derive(Debug, Clone, Copy)]
pub struct OverviewWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub bucket: OverviewBucket,
}

/// Read the three overview aggregates from the rollup carrier. There is no
/// process response cache: a clear or retention drop must be visible to the
/// next read on every node, and the rollup makes the aggregate queries cheap.
pub async fn request_records_overview(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
    perspective: RequestRecordOverviewPerspective,
    mcp_scope: RequestRecordMcpScope,
) -> Result<RequestRecordOverviewResponse> {
    let (summary, trend, breakdown) = tokio::try_join!(
        queries::query_summary(
            pool,
            visible_user_id,
            request_category,
            window,
            user,
            mcp_scope
        ),
        queries::query_trend(
            pool,
            visible_user_id,
            request_category,
            window,
            user,
            mcp_scope
        ),
        queries::query_breakdown(
            pool,
            visible_user_id,
            request_category,
            window,
            user,
            perspective,
            mcp_scope,
        ),
    )?;

    Ok(RequestRecordOverviewResponse {
        summary,
        trend,
        breakdown,
        trend_granularity: window.bucket.as_str().to_string(),
    })
}
