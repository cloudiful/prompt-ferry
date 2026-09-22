use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    db::{get_json_setting, set_json_setting},
    worker_admin_types::CacheAlertSettings,
};

use super::overview::overview_cache_rate;

/// `worker_settings` JSON key holding the persisted [`CacheAlertSettings`].
pub const CACHE_ALERT_SETTINGS_KEY: &str = "cache_alert";

/// A conversation whose recent completed turns keep a low fold-aware cache
/// read rate. Carries only conversation metadata, never user content.
#[derive(Debug, Clone, PartialEq)]
pub struct LowCacheConversation {
    pub conversation_id: Uuid,
    pub model: Option<String>,
    pub turns: i32,
    pub cache_rate: Option<f64>,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct CandidateRow {
    conversation_id: Uuid,
    model: Option<String>,
    turns: i32,
    cache_read: i64,
    full_input: i64,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
}

/// Conversations with at least `min_turns` completed turns inside the
/// `window_minutes` window whose fold-aware cache read rate (`cache_read /
/// full_input`, the same denominator as the usage overview) stays below
/// `threshold`. Candidates are ordered by rate ascending so the worst
/// conversation is alerted first; conversations without a usable token
/// denominator are skipped instead of reported as `0%`.
pub async fn find_low_cache_conversations(
    pool: &PgPool,
    window_minutes: i32,
    min_turns: i32,
    threshold: f64,
) -> Result<Vec<LowCacheConversation>> {
    let rows = sqlx::query_file_as!(
        CandidateRow,
        "src/sql/usage/cache_alert_candidates.sql",
        window_minutes,
        min_turns,
    )
    .fetch_all(pool)
    .await?;

    Ok(low_cache_conversations(rows, threshold))
}

fn low_cache_conversations(rows: Vec<CandidateRow>, threshold: f64) -> Vec<LowCacheConversation> {
    let mut conversations: Vec<LowCacheConversation> = rows
        .into_iter()
        .filter_map(|row| {
            let cache_rate = overview_cache_rate(row.full_input, row.cache_read)?;
            (cache_rate < threshold).then_some(LowCacheConversation {
                conversation_id: row.conversation_id,
                model: row.model,
                turns: row.turns,
                cache_rate: Some(cache_rate),
                window_start: row.window_start,
                window_end: row.window_end,
            })
        })
        .collect();
    conversations.sort_by(|left, right| {
        left.cache_rate
            .partial_cmp(&right.cache_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    conversations
}

pub async fn get_cache_alert_settings(pool: &PgPool) -> Result<CacheAlertSettings> {
    Ok(
        get_json_setting::<CacheAlertSettings>(pool, CACHE_ALERT_SETTINGS_KEY)
            .await?
            .unwrap_or_default()
            .normalized(),
    )
}

pub async fn set_cache_alert_settings(
    pool: &PgPool,
    value: &CacheAlertSettings,
) -> Result<CacheAlertSettings> {
    let normalized = value.clone().normalized();
    set_json_setting(pool, CACHE_ALERT_SETTINGS_KEY, &normalized).await?;
    Ok(normalized)
}

/// Cooldown clock for one conversation; `None` means it was never alerted.
pub async fn last_cache_alert_at(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<Option<DateTime<Utc>>> {
    Ok(
        sqlx::query_file!("src/sql/usage/get_cache_alert_state.sql", conversation_id)
            .fetch_optional(pool)
            .await?
            .map(|row| row.last_alerted_at),
    )
}

pub async fn record_cache_alert(
    pool: &PgPool,
    conversation_id: Uuid,
    cache_rate: f64,
    turns: i32,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/usage/upsert_cache_alert_state.sql",
        conversation_id,
        cache_rate,
        turns,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CandidateRow, low_cache_conversations};
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn candidate(cache_read: i64, full_input: i64, turns: i32) -> CandidateRow {
        CandidateRow {
            conversation_id: Uuid::new_v4(),
            model: Some("gpt-test".to_string()),
            turns,
            cache_read,
            full_input,
            window_start: Utc.with_ymd_and_hms(2026, 9, 21, 10, 0, 0).unwrap(),
            window_end: Utc.with_ymd_and_hms(2026, 9, 21, 10, 30, 0).unwrap(),
        }
    }

    #[test]
    fn keeps_only_conversations_below_the_threshold_sorted_by_rate() {
        let healthy = candidate(9_000, 10_000, 5);
        let worst = candidate(100, 10_000, 6);
        let borderline = candidate(3_000, 10_000, 7);

        let conversations = low_cache_conversations(vec![healthy, worst, borderline], 0.5);

        assert_eq!(conversations.len(), 2);
        assert_eq!(conversations[0].cache_rate, Some(0.01));
        assert_eq!(conversations[1].cache_rate, Some(0.3));
        assert_eq!(conversations[0].turns, 6);
    }

    #[test]
    fn skips_conversations_without_a_usable_denominator_instead_of_reporting_zero() {
        // Fold-aware rate is `None` when `full_input <= 0`; those rows must not
        // be treated as a 0% hit rate and must not trigger an alert.
        let conversations = low_cache_conversations(vec![candidate(0, 0, 5)], 0.2);

        assert!(conversations.is_empty());
    }

    #[test]
    fn clamps_the_rate_to_one_so_over_reader_rows_never_alert() {
        // A malformed row with more cache reads than full input clamps to 1.0
        // and never falls below a sane threshold.
        let conversations = low_cache_conversations(vec![candidate(50_000, 10_000, 5)], 0.2);

        assert!(conversations.is_empty());
    }
}
