use super::*;
use crate::openapi_export::schemas::ErrorEnvelope;

#[utoipa::path(
    get,
    path = "/api/v1/node",
    responses(
        (status = 200, body = NodeStatusResponse, description = "Processing node and authenticated peers"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 403, body = ErrorEnvelope, description = "Administrator required"),
        (status = 503, body = ErrorEnvelope, description = "Node status unavailable")
    ),
    tag = "node"
)]
pub(super) async fn node_status(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::Node).await {
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
