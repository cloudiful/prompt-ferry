use anyhow::Result;
use chrono::{DateTime, Duration, Timelike, Utc};
use sqlx::PgPool;

use crate::db::AffectedRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshBounds {
    /// Inclusive lower bound; `None` refreshes from the beginning.
    pub start: Option<DateTime<Utc>>,
    /// Exclusive whole-minute end.
    pub end: DateTime<Utc>,
}

fn floor_minute(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(value)
}

/// Exclusive whole-minute end of an inclusive affected range, so a row at an
/// exact minute edge still includes its bucket.
fn exclusive_minute_end(end: DateTime<Utc>) -> DateTime<Utc> {
    let after_last = end + Duration::microseconds(1);
    let floor_after_last = floor_minute(after_last);
    if after_last == floor_after_last {
        floor_after_last
    } else {
        floor_after_last + Duration::minutes(1)
    }
}

/// Whole-minute refresh bounds from an inclusive affected range.
///
/// The start floors to its minute; the exclusive end ceilings to a minute.
/// Both ends are capped at the DB current minute: a refresh must never
/// materialize an incomplete current bucket or anything at or beyond the
/// current minute, because that would advance the continuous aggregate's
/// watermark past the present and hide new current-minute writes from the
/// real-time tail.
pub fn refresh_bounds(
    range: AffectedRange,
    current_minute: DateTime<Utc>,
) -> Option<RefreshBounds> {
    let cap = floor_minute(current_minute);
    let end = exclusive_minute_end(range.end).min(cap);
    let start = range.start.map(floor_minute);
    if let Some(start) = start
        && start >= end
    {
        return None;
    }
    Some(RefreshBounds { start, end })
}

/// Refresh the fixed minute rollup after metadata mutations committed.
/// `None` means no metadata instant changed, so no refresh is issued.
pub async fn refresh_usage_aggregate(pool: &PgPool, range: Option<AffectedRange>) -> Result<()> {
    let Some(range) = range else {
        return Ok(());
    };
    let current_minute = sqlx::query_file_scalar!("src/sql/usage/aggregate_refresh_window.sql")
        .fetch_one(pool)
        .await?;
    let Some(bounds) = refresh_bounds(range, current_minute) else {
        return Ok(());
    };
    sqlx::query_file!(
        "src/sql/usage/refresh_usage_aggregate.sql",
        bounds.start,
        bounds.end,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("valid instant")
            .with_timezone(&Utc)
    }

    fn range(start: Option<&str>, end: &str) -> AffectedRange {
        AffectedRange {
            start: start.map(at),
            end: at(end),
        }
    }

    #[test]
    fn empty_and_capped_windows_skip_refresh() {
        let now = at("2026-09-24T10:07:20Z");
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:07:00Z"), "2026-09-24T10:07:00Z"),
                now
            ),
            None
        );
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:08:00Z"), "2026-09-24T10:09:00Z"),
                now
            ),
            None
        );
    }

    #[test]
    fn unbounded_past_refreshes_from_the_beginning() {
        let now = at("2026-09-24T10:07:20Z");
        assert_eq!(
            refresh_bounds(range(None, "2026-09-24T09:12:30Z"), now),
            Some(RefreshBounds {
                start: None,
                end: at("2026-09-24T09:13:00Z"),
            })
        );
    }

    #[test]
    fn a_row_at_an_exact_minute_edge_includes_its_bucket() {
        let now = at("2026-09-24T10:07:20Z");
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T09:05:10Z"), "2026-09-24T09:05:00Z"),
                now
            ),
            Some(RefreshBounds {
                start: Some(at("2026-09-24T09:05:00Z")),
                end: at("2026-09-24T09:06:00Z"),
            })
        );
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T09:05:10Z"), "2026-09-24T09:05:59.999999Z"),
                now
            ),
            Some(RefreshBounds {
                start: Some(at("2026-09-24T09:05:00Z")),
                end: at("2026-09-24T09:06:00Z"),
            })
        );
    }

    #[test]
    fn the_current_incomplete_minute_is_never_refreshed() {
        let now = at("2026-09-24T10:07:20Z");
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:06:30Z"), "2026-09-24T10:07:10Z"),
                now
            ),
            Some(RefreshBounds {
                start: Some(at("2026-09-24T10:06:00Z")),
                end: at("2026-09-24T10:07:00Z"),
            })
        );
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:07:00Z"), "2026-09-24T10:07:30Z"),
                now
            ),
            None
        );
    }

    #[test]
    fn ranges_at_or_after_the_current_minute_never_refresh() {
        let now = at("2026-09-24T10:07:20Z");
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:08:00Z"), "2026-09-24T10:09:00Z"),
                now
            ),
            None
        );
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:07:00Z"), "2026-09-24T10:07:59Z"),
                now
            ),
            None
        );
        assert_eq!(
            refresh_bounds(
                range(Some("2026-09-24T10:05:10Z"), "2026-09-24T10:09:00Z"),
                now
            ),
            Some(RefreshBounds {
                start: Some(at("2026-09-24T10:05:00Z")),
                end: at("2026-09-24T10:07:00Z"),
            })
        );
        assert_eq!(
            refresh_bounds(range(None, "2026-09-25T00:00:00Z"), now),
            Some(RefreshBounds {
                start: None,
                end: at("2026-09-24T10:07:00Z"),
            })
        );
    }
}
