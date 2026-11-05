use super::*;

#[cfg(test)]
mod tests;

const HISTORY_PAGE_DEFAULT: u16 = 50;
const HISTORY_PAGE_MAX: u16 = 200;
/// Upper bound on distinct keys a single request may select; matches the
/// trend display cap so a selected key is never silently dropped.
const KEY_FILTER_MAX: usize = 24;

pub(super) async fn quota_key_observation_history(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Query(query): Query<QuotaKeyObservationHistoryQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let (limit, before_id, key_ids) = match parse_observation_query(&query) {
        Ok(parsed) => parsed,
        Err(ObservationQueryError::InvalidLimit) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_limit",
                "limit must be between 1 and 200",
            );
        }
        Err(ObservationQueryError::InvalidCursor) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "before_id must be a positive observation ID",
            );
        }
        Err(ObservationQueryError::InvalidKey) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_key_id",
                "key_id must be a UUID",
            );
        }
        Err(ObservationQueryError::InvalidKeyIds) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_key_ids",
                "key_ids must be a comma-separated list of up to 24 UUIDs",
            );
        }
        Err(ObservationQueryError::ConflictingKeyFilters) => {
            return error(
                StatusCode::BAD_REQUEST,
                "conflicting_key_filters",
                "key_id and key_ids cannot be combined",
            );
        }
        Err(ObservationQueryError::InvalidKinds) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_kinds",
                "kinds must be a comma-separated list of quota window kinds",
            );
        }
    };
    if state.config_repository.is_sqlite() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "quota_history_unavailable",
            "quota history requires PostgreSQL",
        );
    }

    let endpoint = match db::get_endpoint(&state.pool, endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(_) => return observation_storage_error(endpoint_id),
    };
    // Generic endpoints have no quota normalizer and therefore no
    // per-key observation identity; they read as no history endpoint.
    if endpoint.provider == db::EndpointProvider::Generic {
        return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found");
    }
    let rows = match db::quota_key_observations::list_endpoint_observations(
        &state.pool,
        endpoint_id,
        key_ids.as_deref(),
        before_id,
        i64::from(limit) + 1,
    )
    .await
    {
        Ok(rows) => rows,
        Err(_) => return observation_storage_error(endpoint_id),
    };
    let has_more = rows.len() > usize::from(limit);
    let mut items = Vec::with_capacity(rows.len().min(usize::from(limit)));
    for row in rows.into_iter().take(usize::from(limit)) {
        items.push(QuotaKeyObservationHistoryItem {
            observation_id: row.observation_id.to_string(),
            key_id: row.key_id,
            key_label: row.key_label.clone(),
            provider: row.provider.as_str().to_string(),
            observed_at: row.observed_at,
            ok: row.ok,
            status: row.status.clone(),
            error_code: row.error_code.clone(),
            plan_type: row.plan_type.clone(),
            limit_reached: row.limit_reached,
            windows: row.windows.clone(),
            source: row.source.as_str().to_string(),
        });
    }
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.observation_id.clone()))
        .flatten();
    Json(QuotaKeyObservationHistoryResponse {
        items,
        next_cursor,
        retention_days: db::quota_key_observations::OBSERVATION_RETENTION_DAYS as u16,
    })
    .into_response()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationQueryError {
    InvalidLimit,
    InvalidCursor,
    InvalidKey,
    InvalidKeyIds,
    ConflictingKeyFilters,
    InvalidKinds,
}

fn parse_observation_query(
    query: &QuotaKeyObservationHistoryQuery,
) -> Result<(u16, Option<i64>, Option<Vec<Uuid>>), ObservationQueryError> {
    let limit = query.limit.unwrap_or(HISTORY_PAGE_DEFAULT);
    if !(1..=HISTORY_PAGE_MAX).contains(&limit) {
        return Err(ObservationQueryError::InvalidLimit);
    }
    let before_id = match query.before_id.as_deref() {
        Some(cursor) => match cursor.parse::<i64>() {
            Ok(observation_id) if observation_id > 0 => Some(observation_id),
            _ => return Err(ObservationQueryError::InvalidCursor),
        },
        None => None,
    };
    let key_ids = parse_key_filters(query.key_id.as_deref(), query.key_ids.as_deref())?;
    Ok((limit, before_id, key_ids))
}

/// One request may name a single `key_id` or a comma-separated `key_ids` list,
/// never both. Duplicates collapse; an empty, malformed, or over-cap list is
/// rejected so the bounded query never silently ignores a selected key.
fn parse_key_filters(
    key_id: Option<&str>,
    key_ids: Option<&str>,
) -> Result<Option<Vec<Uuid>>, ObservationQueryError> {
    match (key_id, key_ids) {
        (Some(_), Some(_)) => Err(ObservationQueryError::ConflictingKeyFilters),
        (Some(raw), None) => raw
            .parse::<Uuid>()
            .map(|key_id| Some(vec![key_id]))
            .map_err(|_| ObservationQueryError::InvalidKey),
        (None, Some(raw)) => {
            let mut keys = Vec::new();
            for raw_key in raw.split(',') {
                let raw_key = raw_key.trim();
                if raw_key.is_empty() {
                    return Err(ObservationQueryError::InvalidKeyIds);
                }
                let key_id = raw_key
                    .parse::<Uuid>()
                    .map_err(|_| ObservationQueryError::InvalidKeyIds)?;
                if !keys.contains(&key_id) {
                    keys.push(key_id);
                }
            }
            if keys.is_empty() || keys.len() > KEY_FILTER_MAX {
                return Err(ObservationQueryError::InvalidKeyIds);
            }
            Ok(Some(keys))
        }
        (None, None) => Ok(None),
    }
}

/// Parses the optional comma-separated window kinds against the existing
/// canonical enum, so an unknown or blank value is a request error rather than
/// a silent filter. Duplicates collapse; an absent value means every kind.
fn parse_trend_kinds(
    kinds: Option<&str>,
) -> Result<Option<Vec<CanonicalQuotaWindowKind>>, ObservationQueryError> {
    let Some(raw) = kinds else {
        return Ok(None);
    };
    let mut parsed = Vec::new();
    for raw_kind in raw.split(',') {
        let raw_kind = raw_kind.trim();
        if raw_kind.is_empty() {
            return Err(ObservationQueryError::InvalidKinds);
        }
        let kind = serde_json::from_value::<CanonicalQuotaWindowKind>(serde_json::Value::String(
            raw_kind.to_string(),
        ))
        .map_err(|_| ObservationQueryError::InvalidKinds)?;
        if !parsed.contains(&kind) {
            parsed.push(kind);
        }
    }
    if parsed.is_empty() {
        return Err(ObservationQueryError::InvalidKinds);
    }
    Ok(Some(parsed))
}

/// Bounded, fixed-range trend for the per-key quota observation history. The
/// range and bucket grid are server-owned; only the optional key and kind
/// filters and the named range cross the boundary.
pub(super) async fn quota_key_observation_trend(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Query(query): Query<QuotaKeyObservationTrendQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let key_ids = match parse_key_filters(query.key_id.as_deref(), query.key_ids.as_deref()) {
        Ok(key_ids) => key_ids,
        Err(ObservationQueryError::InvalidKey) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_key_id",
                "key_id must be a UUID",
            );
        }
        Err(ObservationQueryError::InvalidKeyIds) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_key_ids",
                "key_ids must be a comma-separated list of up to 24 UUIDs",
            );
        }
        Err(ObservationQueryError::ConflictingKeyFilters) => {
            return error(
                StatusCode::BAD_REQUEST,
                "conflicting_key_filters",
                "key_id and key_ids cannot be combined",
            );
        }
        Err(_) => unreachable!("key parsing only returns key filter errors"),
    };
    let kinds = match parse_trend_kinds(query.kinds.as_deref()) {
        Ok(kinds) => kinds,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_kinds",
                "kinds must be a comma-separated list of quota window kinds",
            );
        }
    };
    let range = match db::quota_key_observations::TrendRange::parse(query.range.as_deref()) {
        Some(range) => range,
        None => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_range",
                "range must be one of 24h, 7d, 30d",
            );
        }
    };
    if state.config_repository.is_sqlite() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "quota_history_unavailable",
            "quota history requires PostgreSQL",
        );
    }
    // Generic endpoints have no quota normalizer and therefore no per-key
    // observation identity; they read as no trend endpoint.
    match db::get_endpoint(&state.pool, endpoint_id).await {
        Ok(Some(endpoint)) if endpoint.provider != db::EndpointProvider::Generic => {}
        Ok(_) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(_) => return observation_storage_error(endpoint_id),
    }

    // One snapshot of "now" builds the whole half-open window.
    let to = Utc::now();
    let from = to - range.duration();

    let keys = match db::quota_key_observations::load_trend_keys(
        &state.pool,
        endpoint_id,
        from,
        to,
        key_ids.as_deref(),
    )
    .await
    {
        Ok(keys) => keys,
        Err(_) => return observation_storage_error(endpoint_id),
    };
    let keys_truncated = keys.len() > db::quota_key_observations::TREND_KEY_LIMIT;
    let selected_keys: Vec<Uuid> = keys
        .into_iter()
        .take(db::quota_key_observations::TREND_KEY_LIMIT)
        .collect();

    let snapshots = if selected_keys.is_empty() {
        Vec::new()
    } else {
        match db::quota_key_observations::load_trend_buckets(
            &state.pool,
            endpoint_id,
            from,
            to,
            range.bucket_seconds(),
            &selected_keys,
        )
        .await
        {
            Ok(snapshots) => snapshots,
            Err(_) => return observation_storage_error(endpoint_id),
        }
    };

    let projection = db::quota_key_observations::project_trend(
        range.bucket_count(),
        snapshots,
        kinds.as_deref(),
    );
    let bucket_seconds = range.bucket_seconds();
    let series = projection
        .series
        .into_iter()
        .map(|series| QuotaKeyObservationTrendSeries {
            key_id: series.key_id,
            key_label: series.key_label,
            source_id: series.source_id,
            kind: series.kind,
            unit: series.unit,
            metric: match series.metric {
                db::quota_key_observations::TrendMetric::Amount => {
                    QuotaKeyObservationTrendMetric::Amount
                }
                db::quota_key_observations::TrendMetric::RemainingPercent => {
                    QuotaKeyObservationTrendMetric::RemainingPercent
                }
            },
            points: series
                .points
                .into_iter()
                .enumerate()
                .map(|(index, point)| QuotaKeyObservationTrendPoint {
                    at: from + chrono::Duration::seconds(bucket_seconds * index as i64),
                    observed_at: point.observed_at,
                    value: point.value,
                })
                .collect(),
        })
        .collect();

    Json(QuotaKeyObservationTrendResponse {
        from,
        to,
        range: range.as_str().to_string(),
        bucket_seconds,
        bucket_count: range.bucket_count() as u32,
        keys_truncated,
        series_truncated: projection.series_truncated,
        sampled_observations: projection.sampled_observations as u32,
        failed_observations: projection.failed_observations as u32,
        series,
    })
    .into_response()
}

fn observation_storage_error(endpoint_id: Uuid) -> Response {
    tracing::warn!(endpoint_id = %endpoint_id, "quota observation history query failed");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "quota_history_unavailable",
        "quota history is temporarily unavailable",
    )
}
