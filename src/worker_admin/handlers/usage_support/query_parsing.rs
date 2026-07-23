use super::*;

type UsageDateRange = (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
);

pub(in crate::worker_admin::handlers) fn parse_usage_date_range(
    value: &str,
) -> Result<UsageDateRange, Response> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        )
    })?;
    let Some(start_naive) = date.and_hms_opt(0, 0, 0) else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        ));
    };
    let Some(end_naive) = date.succ_opt().and_then(|next| next.and_hms_opt(0, 0, 0)) else {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_date",
            "date must be in YYYY-MM-DD format",
        ));
    };
    Ok((Some(start_naive.and_utc()), Some(end_naive.and_utc())))
}

pub(in crate::worker_admin::handlers) fn parse_overview_window(
    range: Option<RequestRecordOverviewRange>,
    start: Option<chrono::DateTime<chrono::Utc>>,
    end: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<db::OverviewWindow, Response> {
    let now = chrono::Utc::now();
    let range = range.unwrap_or(RequestRecordOverviewRange::Last24h);
    let (start, end) = match range {
        RequestRecordOverviewRange::Last24h => (Some(now - chrono::Duration::hours(24)), Some(now)),
        RequestRecordOverviewRange::Last7d => (Some(now - chrono::Duration::days(7)), Some(now)),
        RequestRecordOverviewRange::Last30d => (Some(now - chrono::Duration::days(30)), Some(now)),
        RequestRecordOverviewRange::Custom => {
            let Some(start) = start else {
                return Err(bad_request("custom overview range requires start"));
            };
            let Some(end) = end else {
                return Err(bad_request("custom overview range requires end"));
            };
            if start >= end {
                return Err(bad_request("overview start must be before end"));
            }
            (Some(start), Some(end))
        }
    };
    let bucket = if end
        .zip(start)
        .map(|(end, start)| end - start <= chrono::Duration::days(3))
        .unwrap_or(true)
    {
        db::OverviewBucket::Hour
    } else {
        db::OverviewBucket::Day
    };
    Ok(db::OverviewWindow { start, end, bucket })
}

pub(in crate::worker_admin::handlers) fn parse_usage_summary_days(
    days: Option<i64>,
) -> Result<i64, Response> {
    match days.unwrap_or(1) {
        1 | 7 | 30 => Ok(days.unwrap_or(1)),
        _ => Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_days",
            "days must be 1, 7, or 30",
        )),
    }
}

pub(in crate::worker_admin::handlers) fn parse_usage_series_bucket(
    bucket: Option<String>,
) -> Result<String, Response> {
    let bucket = bucket.unwrap_or_else(|| "hour".to_string());
    if matches!(bucket.as_str(), "minute" | "hour" | "day") {
        Ok(bucket)
    } else {
        Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_bucket",
            "bucket must be minute, hour, or day",
        ))
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
) -> Result<db::UsageClearQuery, Response> {
    if body
        .start_at
        .zip(body.end_at)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_range",
            "start_at must be earlier than or equal to end_at",
        ));
    }
    if !body.delete_all.unwrap_or(false) && body.start_at.is_none() && body.end_at.is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "range_required",
            "provide start_at/end_at or set delete_all=true",
        ));
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
                return Err(error(StatusCode::FORBIDDEN, "forbidden", "admin required"));
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
                return Err(error(StatusCode::FORBIDDEN, "forbidden", "admin required"));
            }
            let Some(target_user_id) = body.user_id else {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "user_required",
                    "user_id is required",
                ));
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

    use super::{
        parse_overview_window, parse_usage_date_range, parse_usage_series_bucket,
        parse_usage_summary_days,
    };
    use crate::worker_admin::types::RequestRecordOverviewRange;

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
    fn rejects_invalid_usage_summary_days() {
        let error = parse_usage_summary_days(Some(2)).unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn rejects_invalid_usage_series_bucket() {
        let error = parse_usage_series_bucket(Some("week".to_string())).unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }
}
