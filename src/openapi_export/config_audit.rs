use crate::db::config_repository::ConfigAuditPage;
use crate::worker_admin_types::ConfigAuditPageQuery;

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
pub(super) fn list_config_audit() {}
