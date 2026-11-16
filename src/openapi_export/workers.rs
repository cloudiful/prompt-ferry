use crate::worker_admin_types::WorkerListResponse;

#[utoipa::path(
    get,
    path = "/api/v1/admin/workers",
    params(crate::worker_admin_types::TablePageQuery),
    responses(
        (status = 200, body = WorkerListResponse, description = "Workers registered against this database"),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Administrator access required"),
    ),
    tag = "workers"
)]
pub(super) fn list_workers() {}
