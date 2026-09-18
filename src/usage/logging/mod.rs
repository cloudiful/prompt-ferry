//! Usage event ledger for AI/MCP requests.
//!
//! Issue #502 Task 6: `/v1/responses/compact` is recorded under its own
//! request path verbatim (prompt normalization and request text share the
//! `/v1/responses` bore); no path remapping happens here.
mod inference;
mod models;
mod persistence;

pub use models::{UsageLog, UsageRedactionSummary, UsageRequestMetadata};
pub use persistence::record_usage_event;

pub(crate) use models::StandaloneUsageSummary;
pub(crate) use persistence::{UsageRecordingMode, usage_recording_mode};

#[cfg(test)]
mod tests;
