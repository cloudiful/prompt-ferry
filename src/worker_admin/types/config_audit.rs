//! Types for the administrator configuration archive audit trail.
//!
//! The list endpoint is read-only and paginated; `ConfigAuditPage` itself is
//! the response body, so only the query window needs a local type.

use serde::Deserialize;
use utoipa::IntoParams;

/// Default page size when the caller does not ask for one. The repository
/// clamps both the start and the size, so a hostile window stays bounded.
pub const DEFAULT_AUDIT_PAGE_ROWS: i64 = 50;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ConfigAuditPageQuery {
    /// Zero-based offset into the trail, ordered newest first.
    pub first: Option<i64>,
    /// Maximum number of entries to return.
    pub rows: Option<i64>,
}

impl ConfigAuditPageQuery {
    pub fn first(&self) -> i64 {
        self.first.unwrap_or(0)
    }

    pub fn rows(&self) -> i64 {
        self.rows.unwrap_or(DEFAULT_AUDIT_PAGE_ROWS)
    }
}
