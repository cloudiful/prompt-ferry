use super::*;

pub(super) async fn list_workers(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(pool) = state.config_repository.as_postgres() else {
        return state.capability_unavailable(db::Capability::Workers);
    };
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match db::list_workers(pool, first, rows).await {
        Ok((total, online_count, workers)) => {
            let live_worker_count = db::count_live_workers(pool, None)
                .await
                .unwrap_or(online_count);
            let shared_valkey_configured = state.shared_valkey.is_configured();
            Json(WorkerListResponse {
                workers: workers.into_iter().map(WorkerSummary::from).collect(),
                total,
                first,
                rows,
                online_count,
                coordination: WorkerCoordinationState {
                    mode: if shared_valkey_configured {
                        "valkey".to_string()
                    } else {
                        "postgres".to_string()
                    },
                    shared_valkey_configured,
                    degraded: !shared_valkey_configured,
                    degraded_reason: (!shared_valkey_configured).then(|| {
                        "no shared Valkey is configured, so only a single worker incarnation can run"
                            .to_string()
                    }),
                    live_worker_count,
                },
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}
