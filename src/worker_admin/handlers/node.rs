use super::*;

pub(super) async fn node_status(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(status) = &state.node_status else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "node status unavailable",
        );
    };
    let status = status.lock().unwrap_or_else(|e| e.into_inner());
    let mut response = status.own.clone();
    response.peers = status
        .peers
        .values()
        .map(|(_, peer)| peer.clone())
        .collect();
    response.peers.sort_by_key(|peer| peer.node_id);
    Json(response).into_response()
}
