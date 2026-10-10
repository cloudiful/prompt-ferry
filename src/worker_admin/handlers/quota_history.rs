use super::*;

#[cfg(test)]
mod tests;

const HISTORY_PAGE_DEFAULT: u16 = 50;
const HISTORY_PAGE_MAX: u16 = 200;

pub(super) async fn quota_snapshot_history(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Query(query): Query<QuotaSnapshotHistoryQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let (limit, before_id) = match parse_history_query(query) {
        Ok(parsed) => parsed,
        Err(HistoryQueryError::InvalidLimit) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_limit",
                "limit must be between 1 and 200",
            );
        }
        Err(HistoryQueryError::InvalidCursor) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "before_id must be a positive snapshot ID",
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

    let endpoint = match db::quota_snapshots::quota_history_endpoint(&state.pool, endpoint_id).await
    {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(_) => return history_storage_error(endpoint_id),
    };
    if endpoint.provider != db::EndpointProvider::OpenAi.as_str() {
        return error(
            StatusCode::BAD_REQUEST,
            "unsupported_provider",
            "quota history is only available for OpenAI endpoints",
        );
    }
    let rows = match db::quota_snapshots::list_snapshot_history(
        &state.pool,
        endpoint_id,
        before_id,
        i64::from(limit) + 1,
    )
    .await
    {
        Ok(rows) => rows,
        Err(_) => return history_storage_error(endpoint_id),
    };
    let has_more = rows.len() > usize::from(limit);
    let mut items = Vec::with_capacity(rows.len().min(usize::from(limit)));
    for snapshot in rows.into_iter().take(usize::from(limit)) {
        let windows = match serde_json::from_value::<Vec<SubscriptionWindowUsage>>(snapshot.windows)
        {
            Ok(windows) => windows,
            Err(_) => return history_storage_error(endpoint_id),
        };
        items.push(QuotaSnapshotHistoryItem {
            snapshot_id: snapshot.snapshot_id.to_string(),
            observed_at: snapshot.observed_at,
            plan_type: snapshot.plan_type,
            limit_reached: snapshot.limit_reached,
            windows,
            source: snapshot.source.as_str().to_string(),
        });
    }
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.snapshot_id.clone()))
        .flatten();
    Json(QuotaSnapshotHistoryResponse {
        items,
        next_cursor,
        retention_days: db::quota_snapshots::SNAPSHOT_RETENTION_DAYS as u16,
    })
    .into_response()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryQueryError {
    InvalidLimit,
    InvalidCursor,
}

fn parse_history_query(
    query: QuotaSnapshotHistoryQuery,
) -> Result<(u16, Option<i64>), HistoryQueryError> {
    let limit = query.limit.unwrap_or(HISTORY_PAGE_DEFAULT);
    if !(1..=HISTORY_PAGE_MAX).contains(&limit) {
        return Err(HistoryQueryError::InvalidLimit);
    }
    let before_id = match query.before_id {
        Some(cursor) => match cursor.parse::<i64>() {
            Ok(snapshot_id) if snapshot_id > 0 => Some(snapshot_id),
            _ => return Err(HistoryQueryError::InvalidCursor),
        },
        None => None,
    };
    Ok((limit, before_id))
}

fn history_storage_error(endpoint_id: Uuid) -> Response {
    tracing::warn!(endpoint_id = %endpoint_id, "ChatGPT quota history query failed");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "quota_history_unavailable",
        "quota history is temporarily unavailable",
    )
}
