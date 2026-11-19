//! Administrator configuration archive audit trail.
//!
//! `GET /api/v1/admin/config-audit` returns one page of the recorded
//! export/import attempts, newest first. The endpoint is admin-only and
//! read-only: it never writes a row and never returns a passphrase, an archive
//! byte, or a secret value.

use super::*;
use crate::db::config_repository::ConfigAuditPage;

#[utoipa::path(
    get,
    path = "/api/v1/admin/config-audit",
    params(ConfigAuditPageQuery),
    responses(
        (
            status = 200,
            body = ConfigAuditPage,
            description = "One page of recorded configuration export/import attempts, newest first. Non-secret metadata only: the passphrase, the archive bytes, and secret values are never stored or returned."
        ),
        (status = 403, description = "Administrator session required")
    ),
    tag = "config"
)]
pub(super) async fn list_config_audit(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<ConfigAuditPageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::ConfigAudit).await {
        return response.into_response();
    }
    match state
        .config_repository
        .list_config_audit(query.first(), query.rows())
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(err) => internal(&state, err),
    }
}
