use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashSet;

mod refresh;
mod storage;

pub use refresh::{
    acquire_refresh_lease, complete_refresh_failure, complete_refresh_success, get_refresh_state,
};
pub use storage::{
    eligible_endpoints, endpoint_activity, latest_snapshot, list_snapshot_history, list_snapshots,
    prune_expired_snapshots, quota_history_endpoint,
};

pub const SNAPSHOT_RETENTION_DAYS: i64 = 30;
pub const SNAPSHOT_PRUNE_BATCH_SIZE: i64 = 1_000;
pub const SNAPSHOT_PAGE_MAX: i64 = 201;
pub const REFRESH_LEASE_MAX_SECONDS: i64 = 60;

const WINDOW_FIELDS: [&str; 7] = [
    "source_window",
    "window_seconds",
    "used_percent",
    "remaining_percent",
    "reset_at",
    "reset_after_seconds",
    "availability",
];

pub(super) fn validate_windows(value: &Value) -> Result<()> {
    let windows = value
        .as_array()
        .context("ChatGPT quota windows must be a JSON array")?;
    let mut sources = HashSet::new();
    for window in windows {
        let object = window
            .as_object()
            .context("each ChatGPT quota window must be a JSON object")?;
        if object
            .keys()
            .any(|key| !WINDOW_FIELDS.contains(&key.as_str()))
        {
            bail!("ChatGPT quota windows contain an unsupported field");
        }
        let source = object
            .get("source_window")
            .and_then(Value::as_str)
            .context("ChatGPT quota window source is missing")?;
        if !matches!(source, "primary" | "secondary") || !sources.insert(source) {
            bail!("ChatGPT quota window source is invalid or duplicated");
        }
        let availability = object
            .get("availability")
            .and_then(Value::as_str)
            .context("ChatGPT quota window availability is missing")?;
        if !matches!(availability, "known" | "unknown") {
            bail!("ChatGPT quota window availability is invalid");
        }
        let used = optional_percent(object.get("used_percent"), "used_percent")?;
        let remaining = optional_percent(object.get("remaining_percent"), "remaining_percent")?;
        match availability {
            "known" => match (used, remaining) {
                (Some(used), Some(remaining)) if (remaining - (100.0 - used)).abs() <= 1e-9 => {}
                _ => bail!("known ChatGPT quota windows require consistent percentages"),
            },
            "unknown" if used.is_none() && remaining.is_none() => {}
            "unknown" => bail!("unknown ChatGPT quota windows cannot carry percentages"),
            _ => unreachable!(),
        }
        optional_positive_seconds(object.get("window_seconds"), "window_seconds")?;
        optional_nonnegative_seconds(object.get("reset_after_seconds"), "reset_after_seconds")?;
        if let Some(reset_at) = object.get("reset_at") {
            if !reset_at.is_null() {
                let timestamp = reset_at
                    .as_str()
                    .context("ChatGPT quota reset_at must be an RFC3339 timestamp")?;
                chrono::DateTime::parse_from_rfc3339(timestamp)
                    .context("ChatGPT quota reset_at must be an RFC3339 timestamp")?;
            }
        }
    }
    Ok(())
}

fn optional_percent(value: Option<&Value>, field: &str) -> Result<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value
                .as_f64()
                .filter(|number| number.is_finite() && (0.0..=100.0).contains(number))
                .with_context(|| format!("ChatGPT quota {field} must be between 0 and 100"))?;
            Ok(Some(number))
        }
    }
}

fn optional_positive_seconds(value: Option<&Value>, field: &str) -> Result<()> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(value) if value.as_i64().is_some_and(|seconds| seconds > 0) => Ok(()),
        Some(_) => bail!("ChatGPT quota {field} must be positive"),
    }
}

fn optional_nonnegative_seconds(value: Option<&Value>, field: &str) -> Result<()> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(value) if value.as_i64().is_some_and(|seconds| seconds >= 0) => Ok(()),
        Some(_) => bail!("ChatGPT quota {field} must be non-negative"),
    }
}

#[cfg(test)]
pub(super) mod tests;
