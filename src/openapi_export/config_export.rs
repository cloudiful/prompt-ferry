use crate::worker_admin_types::{ConfigExportMetadata, ConfigExportRequest};
#[utoipa::path(
    post,
    path = "/api/v1/admin/config-export",
    request_body = ConfigExportRequest,
    responses(
        (
            status = 200,
            description = "Passphrase-sealed configuration archive (`application/octet-stream`). The response carries the snapshot fingerprint, backend kind, and format version as non-secret headers; the passphrase is never echoed.",
            content_type = "application/octet-stream",
            body = Vec<u8>
        ),
        (status = 400, description = "Passphrase rejected (too short or too long)"),
        (status = 403, description = "Administrator session required")
    ),
    tag = "config"
)]
pub(super) fn export_config() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/config-export/metadata",
    responses(
        (status = 200, body = ConfigExportMetadata, description = "Non-secret export manifest summary for the audit trail"),
        (status = 403, description = "Administrator session required")
    ),
    tag = "config"
)]
pub(super) fn config_export_metadata() {}
