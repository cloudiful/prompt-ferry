//! Issue #759 P1: canonical ChatGPT subscription quota windows.
//!
//! `TokenPlanModelUsage::windows` carries the real `primary_window` /
//! `secondary_window` values for ChatGPT subscription endpoints. Unlike the
//! legacy positional `interval` / `weekly` slots it preserves the reported
//! duration, keeps an unknown usage figure distinct from a genuine `0%`, and
//! never invents a 5-hour or weekly slot for an absent value.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Whether a subscription window carries a usable usage figure. `Unknown` means
/// the window exists upstream but reported no usable percentage, so the display
/// must show an unknown marker instead of a fabricated `0%`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionWindowAvailability {
    Known,
    Unknown,
}

/// One ChatGPT subscription rate-limit window. `source_window` records the
/// upstream slot it came from (`primary` or `secondary`) while `window_seconds`
/// carries the real reported duration, so labels derive from the reported value
/// rather than a positional assumption.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, ToSchema)]
pub struct SubscriptionWindowUsage {
    /// Upstream slot this window came from: `primary` or `secondary`.
    pub source_window: String,
    /// Reported window length in seconds. `None` when the upstream omitted or
    /// reported a non-positive duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<i64>,
    /// Reported used share `0..=100`. `None` when absent or out of range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    /// `100 - used_percent` whenever the used share is known. A genuine
    /// `used_percent = 100` yields `Some(0.0)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_after_seconds: Option<i64>,
    /// `known` when the usage figure is present, `unknown` when the upstream
    /// returned the window but no usable percentage.
    pub availability: SubscriptionWindowAvailability,
}

/// Freshness and durable refresh status for a successful quota observation.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SubscriptionQuotaObservation {
    pub observed_at: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub stale: bool,
    pub last_error_code: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub refreshing: bool,
}
