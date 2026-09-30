use crate::db::config_repository::{ConfigImportApplied, ConfigImportPreview};
use crate::worker_admin_types::ConfigImportRequest;

#[utoipa::path(
    post,
    path = "/api/v1/admin/config-import/preview",
    request_body = ConfigImportRequest,
    responses(
        (
            status = 200,
            body = ConfigImportPreview,
            description = "Per-domain create/update/delete counts and warnings. Read-only; no configuration is written and no payload is returned."
        ),
        (status = 400, description = "Wrong passphrase, tampered archive, or invalid request body"),
        (status = 403, description = "Administrator session required"),
        (status = 413, description = "Archive exceeds the accepted size"),
        (status = 422, description = "Backend mismatch or a domain the target cannot store")
    ),
    tag = "config"
)]
pub(super) fn preview_config_import() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/config-import",
    request_body = ConfigImportRequest,
    responses(
        (
            status = 200,
            body = ConfigImportApplied,
            description = "The archive was validated and applied atomically. Counts and fingerprint only; the passphrase is never echoed."
        ),
        (status = 400, description = "Wrong passphrase, tampered archive, or invalid request body"),
        (status = 403, description = "Administrator session required"),
        (status = 413, description = "Archive exceeds the accepted size"),
        (status = 422, description = "Backend mismatch or a domain the target cannot store")
    ),
    tag = "config"
)]
pub(super) fn import_config() {}
