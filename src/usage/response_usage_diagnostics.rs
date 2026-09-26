//! Debug-only diagnostics for usage on Responses terminal events.
//!
//! An upstream may omit per-request usage, or send a usage object whose keys
//! the shared extractor does not recognize; without a payload record the two
//! cases are indistinguishable. Every terminal Responses event the extractor
//! could not use therefore records one bounded debug line: the fixed event
//! category, whether a usage object was present, and the usage key names.
//! Values, response text, credentials, and identifiers are never part of the
//! record.

use serde_json::{Map, Value};

use super::{extract_usage, truncate_chars};

/// Maximum number of usage key names recorded per terminal event.
pub(super) const MAX_USAGE_KEYS: usize = 16;
/// Maximum characters kept from one usage key name.
pub(super) const MAX_USAGE_KEY_CHARS: usize = 48;

/// Fixed category of the Responses terminal events that can carry usage.
///
/// Transport-level `error` events never carry a usage object (and non-Responses
/// streams reuse the same event name), so they are not diagnosed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalEventCategory {
    Completed,
    Failed,
    Incomplete,
}

impl TerminalEventCategory {
    fn from_event_type(event_type: &str) -> Option<Self> {
        match event_type {
            "response.completed" => Some(Self::Completed),
            "response.failed" => Some(Self::Failed),
            "response.incomplete" => Some(Self::Incomplete),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "response.completed",
            Self::Failed => "response.failed",
            Self::Incomplete => "response.incomplete",
        }
    }
}

/// Presence and parse state of the usage object on a terminal event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UsageState {
    Absent,
    Unrecognized,
}

impl UsageState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Unrecognized => "unrecognized",
        }
    }
}

/// Sanitized record of a terminal event the shared extractor could not use.
///
/// The record carries only a fixed category, the usage state, and key names.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ResponseUsageDiagnosis {
    pub(super) category: TerminalEventCategory,
    pub(super) state: UsageState,
    pub(super) usage_keys: Vec<String>,
}

impl ResponseUsageDiagnosis {
    pub(super) fn record(&self) {
        tracing::debug!(
            category = "responses_usage_diag",
            event = self.category.as_str(),
            usage_state = self.state.as_str(),
            usage_keys = %self.usage_keys.join(","),
            "Responses terminal event without recognized usage"
        );
    }
}

/// Diagnose a terminal Responses event whose usage the shared extractor did
/// not recognize.
///
/// `payload` mirrors the extractor's event payload lookup (the `response`
/// object when the event carries one). Returns `None` for non-terminal events
/// and for terminal events whose usage the extractor already recognizes, so a
/// usable usage object never records a diagnostic.
pub(super) fn diagnose(payload: &Value, value: &Value) -> Option<ResponseUsageDiagnosis> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    let category = TerminalEventCategory::from_event_type(event_type)?;
    if extract_usage(payload)
        .or_else(|| extract_usage(value))
        .is_some()
    {
        return None;
    }
    let usage = usage_object(payload).or_else(|| usage_object(value));
    let state = if usage.is_some() {
        UsageState::Unrecognized
    } else {
        UsageState::Absent
    };
    Some(ResponseUsageDiagnosis {
        category,
        state,
        usage_keys: usage.map(sanitized_keys).unwrap_or_default(),
    })
}

fn usage_object(value: &Value) -> Option<&Map<String, Value>> {
    value.get("usage").and_then(Value::as_object)
}

fn sanitized_keys(usage: &Map<String, Value>) -> Vec<String> {
    let mut keys: Vec<String> = usage.keys().map(|key| sanitize_key(key)).collect();
    keys.sort();
    keys.dedup();
    keys.truncate(MAX_USAGE_KEYS);
    keys
}

/// Keep the `[A-Za-z0-9_.-]` shape of a key name and bound its length; every
/// other character becomes `_` so a hostile key cannot inject log structure.
fn sanitize_key(key: &str) -> String {
    let cleaned: String = key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    truncate_chars(&cleaned, MAX_USAGE_KEY_CHARS)
}
