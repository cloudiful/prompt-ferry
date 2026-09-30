//! Administrator encrypted configuration export.
//!
//! `POST /api/v1/admin/config-export` returns a passphrase-sealed archive of
//! the restorable configuration domains; `POST .../config-export/metadata`
//! returns the non-secret manifest summary an operator records in the audit
//! trail. Both are admin-only (`ensure_admin`) and read-only: no configuration
//! is written and no request/usage/billing/approval/session data is read.
//!
//! The passphrase arrives in the request body, is never logged, never stored,
//! and never appears in a URL, response header, filename, or error message.

use super::*;
use crate::db::config_repository::archive::{ArchiveError, encode_archive, validate_passphrase};
use crate::db::config_repository::{ConfigSnapshot, build_config_snapshot};

pub(super) async fn export_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ConfigExportRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Err(err) = validate_passphrase(&body.passphrase) {
        return archive_error_response(err);
    }
    let snapshot = match build_snapshot(&state).await {
        Ok(snapshot) => snapshot,
        Err(err) => return internal(&state, err),
    };
    let backend = snapshot.manifest.backend_kind;
    let encoded = match encode_archive(&body.passphrase, backend, snapshot) {
        Ok(encoded) => encoded,
        Err(err) => return archive_error_response(err),
    };
    let filename = archive_filename(backend);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .header(
            header::CACHE_CONTROL,
            "no-store, no-cache, must-revalidate, private",
        )
        .header(
            "x-config-export-fingerprint",
            encoded.payload_fingerprint.clone(),
        )
        .header("x-config-export-backend", backend.as_str())
        .header(
            "x-config-export-format-version",
            crate::db::config_repository::SNAPSHOT_FORMAT_VERSION.to_string(),
        )
        .body(encoded.bytes.into())
        .unwrap_or_else(|err| internal(&state, anyhow::anyhow!(err)))
}

/// Non-secret manifest summary: backend, format version, payload fingerprint,
/// and per-domain record/unrecoverable counts. The archive bytes themselves
/// are not produced or returned, and no passphrase is needed (or accepted).
pub(super) async fn config_export_metadata(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let snapshot = match build_snapshot(&state).await {
        Ok(snapshot) => snapshot,
        Err(err) => return internal(&state, err),
    };
    Json(ConfigExportMetadata {
        backend_kind: snapshot.manifest.backend_kind,
        format_version: snapshot.manifest.format_version,
        payload_fingerprint: snapshot.manifest.payload_fingerprint.clone(),
        domains: snapshot
            .manifest
            .domains
            .into_iter()
            .map(|domain| ConfigExportDomainSummary {
                name: domain.name,
                records: domain.records,
                unrecoverable_secrets: domain.unrecoverable_secrets,
            })
            .collect(),
    })
    .into_response()
}

async fn build_snapshot(state: &AdminState) -> anyhow::Result<ConfigSnapshot> {
    build_config_snapshot(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
    )
    .await
}

fn archive_error_response(err: ArchiveError) -> Response {
    let status = match err {
        ArchiveError::PassphraseTooShort { .. } | ArchiveError::PassphraseTooLong { .. } => {
            StatusCode::BAD_REQUEST
        }
        ArchiveError::UnsupportedVersion { .. } | ArchiveError::BackendMismatch { .. } => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ArchiveError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    // The message never contains the passphrase or archive contents.
    error(status, err.code(), &err.to_string())
}

fn archive_filename(backend: crate::db::config_repository::ConfigBackendKind) -> String {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    format!("prompt-ferry-config-{}-{stamp}.pfce", backend.as_str())
}

#[cfg(test)]
mod tests;
