use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::db::{RequestRecordCategory, RequestRecordOverviewResponse};

mod presentation;
mod queries;

pub use self::queries::OverviewBucket;

/// Admin overview responses are cached briefly because the four aggregate
/// queries are the heaviest reads against `request_records` and the dashboard
/// polls them.
const OVERVIEW_CACHE_TTL: Duration = Duration::from_secs(30);
/// `parse_overview_window` derives a fresh `now`-relative window on every
/// request, so cache keys floor the window to this granularity; requests in
/// the same 30s bucket share an entry instead of missing forever.
const OVERVIEW_CACHE_BUCKET_SECS: i64 = 30;

#[derive(Debug, Clone, Copy)]
pub struct OverviewWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub bucket: OverviewBucket,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OverviewCacheKey {
    visible_user_id: Option<i64>,
    request_category: &'static str,
    bucket: OverviewBucket,
    start_bucket: Option<i64>,
    end_bucket: Option<i64>,
    user: Option<String>,
}

type OverviewCache = Mutex<HashMap<OverviewCacheKey, (Instant, RequestRecordOverviewResponse)>>;

fn overview_cache() -> &'static OverviewCache {
    static CACHE: OnceLock<OverviewCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn window_bucket(value: Option<DateTime<Utc>>) -> Option<i64> {
    value.map(|at| at.timestamp().div_euclid(OVERVIEW_CACHE_BUCKET_SECS))
}

fn cached_response(key: &OverviewCacheKey, now: Instant) -> Option<RequestRecordOverviewResponse> {
    let mut cache = overview_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|_, (stored_at, _)| now.duration_since(*stored_at) < OVERVIEW_CACHE_TTL);
    cache.get(key).map(|(_, response)| response.clone())
}

fn store_response(key: OverviewCacheKey, now: Instant, response: &RequestRecordOverviewResponse) {
    let mut cache = overview_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(key, (now, response.clone()));
}

pub async fn request_records_overview(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<RequestRecordOverviewResponse> {
    let key = OverviewCacheKey {
        visible_user_id,
        request_category: request_category.as_str(),
        bucket: window.bucket,
        start_bucket: window_bucket(window.start),
        end_bucket: window_bucket(window.end),
        user: user.map(str::to_owned),
    };
    let now = Instant::now();
    if let Some(cached) = cached_response(&key, now) {
        return Ok(cached);
    }

    let (summary, trend, breakdown, error_breakdown) = tokio::try_join!(
        queries::query_summary(pool, visible_user_id, request_category, window, user),
        queries::query_trend(pool, visible_user_id, request_category, window, user),
        queries::query_breakdown(pool, visible_user_id, request_category, window, user),
        queries::query_error_breakdown(pool, visible_user_id, request_category, window, user),
    )?;

    let response = RequestRecordOverviewResponse {
        summary,
        trend,
        breakdown,
        error_breakdown,
    };
    store_response(key, now, &response);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{RequestRecordOverviewSummary, RequestRecordOverviewTokenUsage};

    fn empty_response() -> RequestRecordOverviewResponse {
        RequestRecordOverviewResponse {
            summary: RequestRecordOverviewSummary {
                request_count: 0,
                success_count: 0,
                error_count: 0,
                method_count: 0,
                success_rate: 0.0,
                p95_total_ms: None,
                p95_first_token_ms: None,
                avg_output_tokens_per_second: None,
                tokens: RequestRecordOverviewTokenUsage {
                    input_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    cache_rate: None,
                    cache_hit_rate: None,
                },
            },
            trend: Vec::new(),
            breakdown: Vec::new(),
            error_breakdown: Vec::new(),
        }
    }

    fn test_key(visible_user_id: i64, user: Option<&str>) -> OverviewCacheKey {
        OverviewCacheKey {
            visible_user_id: Some(visible_user_id),
            request_category: "ai",
            bucket: OverviewBucket::Hour,
            start_bucket: Some(0),
            end_bucket: Some(1),
            user: user.map(str::to_owned),
        }
    }

    #[test]
    fn window_bucket_floors_to_cache_granularity() {
        let at = DateTime::<Utc>::from_timestamp(1_020, 0).expect("valid timestamp");
        let within = DateTime::<Utc>::from_timestamp(1_049, 0).expect("valid timestamp");
        let across = DateTime::<Utc>::from_timestamp(1_050, 0).expect("valid timestamp");

        assert_eq!(window_bucket(Some(at)), Some(1_020 / 30));
        assert_eq!(window_bucket(Some(at)), window_bucket(Some(within)));
        assert_ne!(window_bucket(Some(at)), window_bucket(Some(across)));
        assert_eq!(window_bucket(None), None);
    }

    #[test]
    fn cache_key_includes_user_and_category_scope() {
        assert_ne!(test_key(1, Some("a")), test_key(1, Some("b")));
        assert_ne!(test_key(1, None), test_key(1, Some("a")));
        assert_ne!(test_key(1, None), test_key(2, None));
    }

    #[test]
    fn cache_serves_entries_within_ttl() {
        let key = test_key(i64::MIN, None);
        let now = Instant::now();
        store_response(key.clone(), now, &empty_response());

        assert_eq!(
            cached_response(&key, now).map(|response| response.summary.request_count),
            Some(0)
        );
    }

    #[test]
    fn cache_drops_entries_past_ttl() {
        let key = test_key(i64::MIN + 1, None);
        let stale = Instant::now() - OVERVIEW_CACHE_TTL - Duration::from_secs(1);
        store_response(key.clone(), stale, &empty_response());

        assert!(cached_response(&key, Instant::now()).is_none());
    }
}
