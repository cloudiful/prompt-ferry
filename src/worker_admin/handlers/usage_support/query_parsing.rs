use super::*;
use chrono::Datelike;

type UsageDateRange = (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

pub(in crate::worker_admin::handlers) fn parse_usage_date_range(
    value: &str,
) -> Result<UsageDateRange, Box<Response>> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        ))
    })?;
    let Some(start_naive) = date.and_hms_opt(0, 0, 0) else {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        )));
    };
    let Some(end_naive) = date.succ_opt().and_then(|next| next.and_hms_opt(0, 0, 0)) else {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        )));
    };
    Ok((Some(start_naive.and_utc()), Some(end_naive.and_utc())))
}

/// Combine the legacy `date` day-range with optional `start`/`end` bounds.
///
/// Returns the intersection so a malformed `start`/`end` cannot silently
/// widen the query: missing or empty inputs are ignored, mismatched bounds
/// are rejected, and any effective `start >= end` (including empty
/// intersections between a legacy date and an earlier explicit end) is
/// surfaced as a bad-request error.
pub(in crate::worker_admin::handlers) fn combine_record_date_range(
    date_range: UsageDateRange,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<UsageDateRange, Box<Response>> {
    let (legacy_start, legacy_end) = date_range;
    if start.is_some() && end.is_some() && start >= end {
        return Err(Box::new(bad_request("start must be earlier than end")));
    }
    let effective_start = match (legacy_start, start) {
        (Some(legacy), Some(bound)) => Some(legacy.max(bound)),
        (legacy, bound) => legacy.or(bound),
    };
    let effective_end = match (legacy_end, end) {
        (Some(legacy), Some(bound)) => Some(legacy.min(bound)),
        (legacy, bound) => legacy.or(bound),
    };
    if let (Some(s), Some(e)) = (effective_start, effective_end) {
        if s >= e {
            return Err(Box::new(bad_request("range produces an empty window")));
        }
    }
    Ok((effective_start, effective_end))
}

/// Resolve the effective time bounds for the records list endpoint.
///
/// Explicit `start`/`end` always win over the preset `range`. When `range`
/// is supplied without bounds, the helper applies the existing overview
/// preset semantics so the records list and overview see the same window
/// for the same picker selection. When no picker input is present at all,
/// the helper returns `(None, None)` so legacy `date` (or the historical
/// unbounded default) remains the sole time filter. `Custom` without
/// explicit bounds is rejected.
pub(in crate::worker_admin::handlers) fn resolve_record_range_bounds(
    range: Option<RequestRecordOverviewRange>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<UsageDateRange, Box<Response>> {
    if start.is_some() || end.is_some() {
        if let (Some(s), Some(e)) = (start, end) {
            if s >= e {
                return Err(Box::new(bad_request("start must be earlier than end")));
            }
            return Ok((Some(s), Some(e)));
        }
        return Ok((start, end));
    }
    let Some(range) = range else {
        return Ok((None, None));
    };
    let bounds = match range {
        RequestRecordOverviewRange::Last24h => (Some(now - chrono::Duration::hours(24)), Some(now)),
        RequestRecordOverviewRange::Last7d => (Some(now - chrono::Duration::days(7)), Some(now)),
        RequestRecordOverviewRange::Last30d => (Some(now - chrono::Duration::days(30)), Some(now)),
        RequestRecordOverviewRange::CurrentMonth => {
            let month_start = now
                .date_naive()
                .with_day(1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| date.and_utc())
                .expect("a calendar date always has a valid midnight");
            (Some(month_start), Some(now))
        }
        RequestRecordOverviewRange::Custom => {
            return Err(Box::new(bad_request(
                "custom range requires start and end query parameters",
            )));
        }
    };
    Ok(bounds)
}

pub(in crate::worker_admin::handlers) fn parse_overview_window(
    range: Option<RequestRecordOverviewRange>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<db::OverviewWindow, Box<Response>> {
    let now = chrono::Utc::now();
    let range = range.unwrap_or(RequestRecordOverviewRange::Last24h);
    let (start, end) = match range {
        RequestRecordOverviewRange::Last24h => (Some(now - chrono::Duration::hours(24)), Some(now)),
        RequestRecordOverviewRange::Last7d => (Some(now - chrono::Duration::days(7)), Some(now)),
        RequestRecordOverviewRange::Last30d => (Some(now - chrono::Duration::days(30)), Some(now)),
        RequestRecordOverviewRange::CurrentMonth => {
            let month_start = now
                .date_naive()
                .with_day(1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| date.and_utc())
                .expect("a calendar date always has a valid midnight");
            (Some(month_start), Some(now))
        }
        RequestRecordOverviewRange::Custom => {
            let Some(start) = start else {
                return Err(Box::new(bad_request(
                    "custom overview range requires start",
                )));
            };
            let Some(end) = end else {
                return Err(Box::new(bad_request("custom overview range requires end")));
            };
            if start >= end {
                return Err(Box::new(bad_request("overview start must be before end")));
            }
            (Some(start), Some(end))
        }
    };
    let bucket = match range {
        RequestRecordOverviewRange::CurrentMonth => db::OverviewBucket::Day,
        _ if end
            .zip(start)
            .map(|(end, start)| end - start <= chrono::Duration::days(3))
            .unwrap_or(true) =>
        {
            db::OverviewBucket::Hour
        }
        _ => db::OverviewBucket::Day,
    };
    Ok(db::OverviewWindow { start, end, bucket })
}

pub(in crate::worker_admin::handlers) fn parse_usage_summary_days(
    days: Option<i64>,
) -> Result<i64, Box<Response>> {
    match days.unwrap_or(1) {
        1 | 7 | 30 => Ok(days.unwrap_or(1)),
        _ => Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_days",
            "days must be 1, 7, or 30",
        ))),
    }
}

pub(in crate::worker_admin::handlers) fn parse_usage_series_bucket(
    bucket: Option<String>,
) -> Result<String, Box<Response>> {
    let bucket = bucket.unwrap_or_else(|| "hour".to_string());
    if matches!(bucket.as_str(), "minute" | "hour" | "day") {
        Ok(bucket)
    } else {
        Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_bucket",
            "bucket must be minute, hour, or day",
        )))
    }
}

pub(in crate::worker_admin::handlers) fn build_request_record_query(
    user: &SessionUser,
    query: UsageEventsQuery,
    date_start: Option<chrono::DateTime<chrono::Utc>>,
    date_end: Option<chrono::DateTime<chrono::Utc>>,
) -> db::RequestRecordQuery {
    db::RequestRecordQuery {
        visible_user_id: (!user.is_admin).then_some(user.user_id),
        request_category: query
            .request_category
            .unwrap_or(db::RequestRecordCategory::Ai),
        first: query.first.unwrap_or(0),
        rows: query.rows.unwrap_or(10),
        sort_field: query.sort_field.unwrap_or_else(|| "created_at".to_string()),
        sort_order: query.sort_order.unwrap_or(-1),
        search: query.search,
        date_start,
        date_end,
        client_key_id: query.client_key_id,
        user: query.user,
        model: query.model,
        endpoint_id: query.endpoint_id,
        mcp_server_id: query.mcp_server_id,
        mcp_bearer_token_slot: query.mcp_bearer_token_slot,
        request_state: query.request_state,
        redaction_applied: query.redaction_applied,
    }
}

pub(in crate::worker_admin::handlers) fn build_usage_clear_query(
    user: &SessionUser,
    body: UsageClearRequest,
) -> Result<db::UsageClearQuery, Box<Response>> {
    if body
        .start_at
        .zip(body.end_at)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_range",
            "start_at must be earlier than or equal to end_at",
        )));
    }
    if !body.delete_all.unwrap_or(false) && body.start_at.is_none() && body.end_at.is_none() {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "range_required",
            "provide start_at/end_at or set delete_all=true",
        )));
    }

    Ok(match body.scope.unwrap_or(UsageClearScope::CurrentUser) {
        UsageClearScope::CurrentUser => db::UsageClearQuery {
            scope: db::UsageClearScope::CurrentUser,
            visible_user_id: Some(user.user_id),
            target_user_id: None,
            start_at: body.start_at,
            end_at: body.end_at,
        },
        UsageClearScope::AllUsers => {
            if !user.is_admin {
                return Err(Box::new(error(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "admin required",
                )));
            }
            db::UsageClearQuery {
                scope: db::UsageClearScope::AllUsers,
                visible_user_id: None,
                target_user_id: None,
                start_at: body.start_at,
                end_at: body.end_at,
            }
        }
        UsageClearScope::TargetUser => {
            if !user.is_admin {
                return Err(Box::new(error(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "admin required",
                )));
            }
            let Some(target_user_id) = body.user_id else {
                return Err(Box::new(error(
                    StatusCode::BAD_REQUEST,
                    "user_required",
                    "user_id is required",
                )));
            };
            db::UsageClearQuery {
                scope: db::UsageClearScope::TargetUser,
                visible_user_id: None,
                target_user_id: Some(target_user_id),
                start_at: body.start_at,
                end_at: body.end_at,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use chrono::{Datelike, Timelike};

    use super::{
        combine_record_date_range, parse_overview_window, parse_usage_date_range,
        parse_usage_series_bucket, parse_usage_summary_days, resolve_record_range_bounds,
    };
    use crate::worker_admin::types::RequestRecordOverviewRange;
    use crate::worker_admin_types::SessionUser;

    #[test]
    fn parses_usage_date_into_utc_day_range() {
        let (start, end) = parse_usage_date_range("2026-05-21").expect("date range");
        assert_eq!(start.unwrap().to_rfc3339(), "2026-05-21T00:00:00+00:00");
        assert_eq!(end.unwrap().to_rfc3339(), "2026-05-22T00:00:00+00:00");
    }

    #[test]
    fn rejects_invalid_usage_date() {
        assert!(parse_usage_date_range("2026/05/21").is_err());
    }

    #[test]
    fn custom_range_requires_end_after_start() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2026-05-20T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error = parse_overview_window(
            Some(RequestRecordOverviewRange::Custom),
            Some(start),
            Some(end),
        )
        .unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn current_month_starts_at_first_day() {
        let window =
            parse_overview_window(Some(RequestRecordOverviewRange::CurrentMonth), None, None)
                .expect("current month range");
        assert_eq!(window.start.unwrap().day(), 1);
        assert!(window.end.unwrap() > window.start.unwrap());
        assert!(matches!(window.bucket, crate::db::OverviewBucket::Day));
    }

    #[test]
    fn rejects_invalid_usage_summary_days() {
        let error = parse_usage_summary_days(Some(2)).unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn rejects_invalid_usage_series_bucket() {
        let error = parse_usage_series_bucket(Some("week".to_string())).unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn combine_record_date_range_passes_through_legacy_only() {
        let (start, end) = parse_usage_date_range("2026-05-21").expect("date range");
        let (effective_start, effective_end) =
            combine_record_date_range((start, end), None, None).expect("combine");
        assert_eq!(effective_start, start);
        assert_eq!(effective_end, end);
    }

    #[test]
    fn combine_record_date_range_intersects_legacy_and_explicit_range() {
        let (legacy_start, legacy_end) =
            parse_usage_date_range("2026-05-21").expect("legacy date range");
        let start = chrono::DateTime::parse_from_rfc3339("2026-05-21T06:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2026-05-21T18:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (effective_start, effective_end) =
            combine_record_date_range((legacy_start, legacy_end), Some(start), Some(end))
                .expect("combine");
        assert_eq!(effective_start, Some(start));
        assert_eq!(effective_end, Some(end));
    }

    #[test]
    fn combine_record_date_range_narrows_to_intersection() {
        let (legacy_start, legacy_end) =
            parse_usage_date_range("2026-05-21").expect("legacy date range");
        let later_start = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (effective_start, effective_end) =
            combine_record_date_range((legacy_start, legacy_end), Some(later_start), None)
                .expect("combine");
        assert_eq!(effective_start, Some(later_start));
        assert_eq!(effective_end, legacy_end);
    }

    #[test]
    fn combine_record_date_range_rejects_end_before_start() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2026-05-20T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error = combine_record_date_range((None, None), Some(start), Some(end))
            .expect_err("invalid bounds");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn combine_record_date_range_rejects_empty_intersection() {
        let (legacy_start, legacy_end) =
            parse_usage_date_range("2026-05-21").expect("legacy date range");
        let later_start = chrono::DateTime::parse_from_rfc3339("2026-05-22T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let later_end = chrono::DateTime::parse_from_rfc3339("2026-05-23T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error = combine_record_date_range(
            (legacy_start, legacy_end),
            Some(later_start),
            Some(later_end),
        )
        .expect_err("empty intersection");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn combine_record_date_range_without_legacy_uses_bounds_only() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = chrono::DateTime::parse_from_rfc3339("2026-05-22T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (effective_start, effective_end) =
            combine_record_date_range((None, None), Some(start), Some(end)).expect("combine");
        assert_eq!(effective_start, Some(start));
        assert_eq!(effective_end, Some(end));
    }

    #[test]
    fn build_request_record_query_propagates_client_key_id_and_range() {
        use super::build_request_record_query;
        use crate::worker_admin::types::UsageEventsQuery;
        use chrono::{DateTime, Utc};

        let user = SessionUser {
            user_id: 42,
            login_name: "owner".to_string(),
            display_name: "owner".to_string(),
            is_admin: false,
        };
        let start: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-05-22T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let query = UsageEventsQuery {
            request_category: None,
            range: Some(RequestRecordOverviewRange::Custom),
            first: Some(0),
            rows: Some(25),
            sort_field: Some("created_at".to_string()),
            sort_order: Some(-1),
            search: Some("needle".to_string()),
            date: Some("2026-05-21".to_string()),
            start: Some(start),
            end: Some(end),
            client_key_id: Some(7),
            user: Some("owner".to_string()),
            model: Some("gpt".to_string()),
            endpoint_id: None,
            mcp_server_id: None,
            mcp_bearer_token_slot: None,
            request_state: None,
            redaction_applied: None,
        };
        let request_query = build_request_record_query(&user, query, Some(start), Some(end));
        assert_eq!(request_query.visible_user_id, Some(42));
        assert_eq!(request_query.client_key_id, Some(7));
        assert_eq!(request_query.date_start, Some(start));
        assert_eq!(request_query.date_end, Some(end));
        assert_eq!(request_query.rows, 25);
        assert_eq!(request_query.first, 0);
        assert_eq!(request_query.search.as_deref(), Some("needle"));
        assert_eq!(request_query.user.as_deref(), Some("owner"));
        assert_eq!(request_query.model.as_deref(), Some("gpt"));
    }

    #[test]
    fn resolve_record_range_bounds_returns_unbounded_when_unset() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (start, end) = resolve_record_range_bounds(None, None, None, now).expect("bounds");
        assert!(
            start.is_none() && end.is_none(),
            "no picker input must not introduce an implicit Last24h window"
        );
    }

    #[test]
    fn combine_record_date_range_rejects_empty_intersection_from_legacy_date_and_earlier_end() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (legacy_start, legacy_end) =
            parse_usage_date_range("2026-05-21").expect("legacy date range");
        let earlier_end = now - chrono::Duration::days(3);
        let error = combine_record_date_range((legacy_start, legacy_end), None, Some(earlier_end))
            .expect_err("empty intersection must error");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn resolve_record_range_bounds_applies_preset_windows() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for (range, expected_span) in [
            (
                RequestRecordOverviewRange::Last7d,
                chrono::Duration::days(7),
            ),
            (
                RequestRecordOverviewRange::Last30d,
                chrono::Duration::days(30),
            ),
        ] {
            let (start, end) =
                resolve_record_range_bounds(Some(range), None, None, now).expect("bounds");
            let start = start.unwrap();
            let end = end.unwrap();
            assert_eq!(end, now);
            assert_eq!(now - start, expected_span);
        }
        let (start, end) = resolve_record_range_bounds(
            Some(RequestRecordOverviewRange::CurrentMonth),
            None,
            None,
            now,
        )
        .expect("bounds");
        let start = start.unwrap();
        assert_eq!(start.day(), 1);
        assert_eq!(start.hour(), 0);
        assert_eq!(start.minute(), 0);
        assert_eq!(start.second(), 0);
        assert_eq!(end.unwrap(), now);
    }

    #[test]
    fn resolve_record_range_bounds_prefers_explicit_over_preset() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let custom_start = chrono::DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let custom_end = chrono::DateTime::parse_from_rfc3339("2026-05-21T18:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let (start, end) = resolve_record_range_bounds(
            Some(RequestRecordOverviewRange::Last24h),
            Some(custom_start),
            Some(custom_end),
            now,
        )
        .expect("bounds");
        assert_eq!(start, Some(custom_start));
        assert_eq!(end, Some(custom_end));
    }

    #[test]
    fn resolve_record_range_bounds_rejects_end_before_start() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let bad_start = chrono::DateTime::parse_from_rfc3339("2026-05-21T18:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let bad_end = chrono::DateTime::parse_from_rfc3339("2026-05-21T00:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error = resolve_record_range_bounds(
            Some(RequestRecordOverviewRange::Custom),
            Some(bad_start),
            Some(bad_end),
            now,
        )
        .expect_err("invalid bounds");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn resolve_record_range_bounds_rejects_custom_without_bounds() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-21T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error =
            resolve_record_range_bounds(Some(RequestRecordOverviewRange::Custom), None, None, now)
                .expect_err("custom needs bounds");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }
}
