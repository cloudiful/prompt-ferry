use super::schemas::ErrorEnvelope;
use crate::worker_admin_types::NodeStatusResponse;

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
pub(super) fn node_status() {}
