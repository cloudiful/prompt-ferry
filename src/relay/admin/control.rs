//! The relay's own control endpoints.
//!
//! Every handler here answers from relay state alone. None of them reads a
//! worker, a worker database, or a shared configuration store, which is what
//! makes the management page usable on a relay-only host.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::{
    dto::{RelayHostResponse, RelayHostRoleRequest, RelaySettingsResponse, RelaySettingsUpdate},
    restart,
    state::RelayAdminState,
};

/// Liveness for the management listener itself, with no credential required.
///
/// It answers only that the control plane is accepting connections; every fact
/// about the host is behind authentication.
pub(super) async fn healthz() -> &'static str {
    "ok"
}

pub(super) async fn status(State(state): State<RelayAdminState>) -> Response {
    Json(state.status().await).into_response()
}

pub(super) async fn settings(State(state): State<RelayAdminState>) -> Response {
    Json(state.settings_response()).into_response()
}

pub(super) async fn update_settings(
    State(state): State<RelayAdminState>,
    Json(update): Json<RelaySettingsUpdate>,
) -> Response {
    match state.save_settings(&update) {
        Ok(restart_required) => Json(RelaySettingsResponse {
            restart_required,
            relay: state.settings.clone(),
        })
        .into_response(),
        Err(message) => super::dto_error(StatusCode::BAD_REQUEST, "invalid_setting", message),
    }
}

pub(super) async fn host(State(state): State<RelayAdminState>) -> Response {
    Json(host_response(&state).await).into_response()
}

pub(super) async fn set_role(
    State(state): State<RelayAdminState>,
    Json(body): Json<RelayHostRoleRequest>,
) -> Response {
    if let Err(message) = state.save_role(body.role) {
        return super::dto_error(StatusCode::SERVICE_UNAVAILABLE, "role_not_saved", message);
    }
    tracing::info!(
        role = body.role.as_str(),
        "relay management saved a new host role; it takes effect on the next start"
    );
    Json(host_response(&state).await).into_response()
}

/// Ask this host to stop so the saved settings take effect.
///
/// The process returns from its startup, which is the path a supervisor already
/// restarts through. Nothing is killed mid-request: the response is written
/// first and the signal is raised on a short delay.
pub(super) async fn request_restart(State(_state): State<RelayAdminState>) -> Response {
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        tracing::info!("relay management requested a restart of this host");
        restart::request();
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "restart_requested": true,
            "message": "this host is stopping so the saved settings take effect",
        })),
    )
        .into_response()
}

async fn host_response(state: &RelayAdminState) -> RelayHostResponse {
    let (role, pending_role) = state.reported_role();
    RelayHostResponse {
        role,
        pending_role,
        restart_required: state.restart_pending(),
    }
}
