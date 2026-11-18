use crate::worker_admin_types::NodeListResponse;

#[utoipa::path(
    get,
    path = "/api/v1/admin/nodes",
    params(crate::worker_admin_types::TablePageQuery),
    responses(
        (status = 200, body = NodeListResponse, description = "Database processor node groups"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Administrator access required"),
    ),
    tag = "nodes"
)]
pub(super) fn list_nodes() {}
