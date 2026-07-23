use crate::worker_admin_types::BridgeStatus;

#[utoipa::path(
    get,
    path = "/api/v1/bridge/status",
    responses((status = 200, body = BridgeStatus, description = "Bridge status")),
    tag = "bridge"
)]
pub(super) fn bridge_status() {}
