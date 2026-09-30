//! Types for the admin encrypted configuration import.
//!
//! The request carries the passphrase and the base64-encoded archive in the
//! JSON body — never the query string, an URL, or a header. Both fields are
//! `write_only`, and `Debug` is redacted so an accidental log cannot leak the
//! passphrase or archive bytes.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::db::config_repository::ImportError;
use crate::db::config_repository::archive::MAX_ARCHIVE_BYTES;

/// Base64 expands 3 bytes to 4 characters; bound the encoded string before
/// decoding so a hostile body cannot force a large allocation.
pub const MAX_ARCHIVE_BASE64_LEN: usize = MAX_ARCHIVE_BYTES.div_ceil(3) * 4;

/// Passphrase-encrypted import request.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigImportRequest {
    /// Passphrase that sealed the archive. Never stored, logged, or placed in
    /// a URL.
    #[schema(value_type = String, write_only = true)]
    pub passphrase: String,
    /// Standard base64 of the `.pfce` archive returned by the export endpoint.
    #[schema(value_type = String, write_only = true)]
    pub archive_base64: String,
}

impl std::fmt::Debug for ConfigImportRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigImportRequest")
            .field("passphrase", &"[redacted]")
            .field("archive_base64", &"[redacted]")
            .finish()
    }
}

/// Decode the request archive after enforcing the encoded-size bound.
pub fn decode_archive_base64(archive_base64: &str) -> Result<Vec<u8>, ImportError> {
    if archive_base64.len() > MAX_ARCHIVE_BASE64_LEN {
        return Err(ImportError::TooLarge {
            limit: MAX_ARCHIVE_BASE64_LEN,
        });
    }
    let bytes = STANDARD
        .decode(archive_base64.as_bytes())
        .map_err(|_| ImportError::InvalidArchive("archive body is not valid base64"))?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(ImportError::TooLarge {
            limit: MAX_ARCHIVE_BYTES,
        });
    }
    Ok(bytes)
}
