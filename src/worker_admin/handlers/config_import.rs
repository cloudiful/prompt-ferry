//! Administrator encrypted configuration import.
//!
//! `POST /api/v1/admin/config-import/preview` decrypts an archive and returns
//! the per-domain differences without writing anything;
//! `POST /api/v1/admin/config-import` repeats the validation and replaces the
//! restorable configuration domains in one backend transaction. Both are
//! admin-only (`ensure_admin`).
//!
//! The passphrase and the base64 archive arrive in the JSON body, are never
//! logged, and never appear in an URL, header, filename, or error message.
//! A successful import triggers exactly one runtime snapshot publication; a
//! publication failure never fails the already-committed import.

use super::*;
use crate::db::config_repository::archive::ArchiveError;
use crate::db::config_repository::{
    ImportError, apply_config_import, preview_config_import as preview_import_data,
};
use crate::worker_admin_types::decode_archive_base64;

pub(super) async fn preview_config_import(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ConfigImportRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let bytes = match decode_archive_base64(&body.archive_base64) {
        Ok(bytes) => bytes,
        Err(err) => return import_error_response(err),
    };
    match preview_import_data(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
        &body.passphrase,
        &bytes,
    )
    .await
    {
        Ok(preview) => Json(preview).into_response(),
        Err(err) => import_error_response(err),
    }
}

pub(super) async fn import_config(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ConfigImportRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let bytes = match decode_archive_base64(&body.archive_base64) {
        Ok(bytes) => bytes,
        Err(err) => return import_error_response(err),
    };
    let applied = match apply_config_import(
        &state.config_repository,
        state.relay_secret_manager.as_ref(),
        &body.passphrase,
        &bytes,
    )
    .await
    {
        Ok(applied) => applied,
        Err(err) => return import_error_response(err),
    };
    // The replacement is committed; refresh the runtime once. A publication
    // failure must not fail the import that already landed.
    let _ = publish_snapshot(&state).await;
    Json(applied).into_response()
}

fn import_error_response(err: ImportError) -> Response {
    let status = match &err {
        ImportError::Archive(ArchiveError::PassphraseTooShort { .. })
        | ImportError::Archive(ArchiveError::PassphraseTooLong { .. })
        | ImportError::Archive(ArchiveError::DecryptionFailed)
        | ImportError::InvalidArchive(_) => StatusCode::BAD_REQUEST,
        ImportError::Archive(ArchiveError::UnsupportedVersion { .. })
        | ImportError::Archive(ArchiveError::BackendMismatch { .. })
        | ImportError::UnsupportedDomain { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        ImportError::Archive(ArchiveError::TooLarge { .. }) | ImportError::TooLarge { .. } => {
            StatusCode::PAYLOAD_TOO_LARGE
        }
        // Structural archive failures (bad header, key derivation, codec) are
        // server-side conditions rather than client input errors.
        ImportError::Archive(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ImportError::Write(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let message = match &err {
        // Never echo the underlying persistence error on a committed-write
        // failure; the code is the machine-readable contract.
        ImportError::Write(_) => "configuration import failed".to_string(),
        _ => err.to_string(),
    };
    error(status, err.code(), &message)
}

#[cfg(test)]
mod tests;
