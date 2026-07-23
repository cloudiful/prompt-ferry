use crate::{
    db,
    worker_admin_types::{ApprovalPageQuery, ApprovalPageResponse},
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/approvals",
    params(ApprovalPageQuery),
    responses((status = 200, body = ApprovalPageResponse, description = "Approval page")),
    tag = "approvals"
)]
pub(super) fn list_approvals() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/approvals/{approval_id}",
    params(("approval_id" = uuid::Uuid, Path, description = "Approval ID")),
    responses((status = 200, body = db::ApprovalRequest, description = "Approval detail")),
    tag = "approvals"
)]
pub(super) fn get_approval() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/approvals/{approval_id}/approve",
    params(("approval_id" = uuid::Uuid, Path, description = "Approval ID")),
    responses((status = 200, body = db::ApprovalRequest, description = "Approved request")),
    tag = "approvals"
)]
pub(super) fn approve_approval() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/approvals/{approval_id}/reject",
    params(("approval_id" = uuid::Uuid, Path, description = "Approval ID")),
    responses((status = 200, body = db::ApprovalRequest, description = "Rejected request")),
    tag = "approvals"
)]
pub(super) fn reject_approval() {}
