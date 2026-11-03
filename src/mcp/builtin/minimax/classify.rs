//! The response classification of the builtin MiniMax tools (issue #1179 P1).
//!
//! The MiniMax API reports a spent plan quota as a business code inside a
//! successful HTTP response, so the classification reads the body code as
//! well as the HTTP status. Only a quota code and a rate limit may switch to
//! another key; every other failure is returned to the caller without an
//! automatic replay.
//!
//! A successful status is not a successful call on its own (issue #1179 P2
//! repair): the body must be the documented payload shape of the tool and, when
//! it carries the `base_resp` envelope at all, that envelope must hold a valid
//! integral `status_code`. An absent envelope is accepted only with the
//! documented shape; a present but invalid envelope is always a failure.

use super::Tool;
use super::transport::MiniMaxResponse;
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

/// The MiniMax business codes that report an exhausted plan quota.
pub(super) const EXHAUSTED_STATUS_CODES: [i64; 2] = [2056, 2067];

/// The `base_resp.status_code` of a response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BusinessStatus {
    /// The body carries no `base_resp` envelope at all.
    Absent,
    /// The envelope carries a valid integral status code.
    Valid(i64),
    /// The envelope is present but malformed: a non-object envelope, a
    /// missing field, or a value that is not an exact integer.
    Invalid,
}

/// What one upstream response means for the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// The call succeeded.
    Success,
    /// The plan quota of the tried key is exhausted.
    Exhausted { code: i64 },
    /// The upstream rate limited the call.
    RateLimited { retry_after: Option<Duration> },
    /// A non-success HTTP status.
    Failed { status: u16 },
    /// A non-zero business code inside a successful HTTP response.
    ProviderFailed { code: i64 },
    /// The response is not the documented payload of the tool.
    Malformed,
}

/// Classify one upstream response.
pub(super) fn classify(response: &MiniMaxResponse, tool: Tool) -> Outcome {
    if response.status == StatusCode::TOO_MANY_REQUESTS {
        return Outcome::RateLimited {
            retry_after: response.retry_after,
        };
    }
    if !response.status.is_success() {
        return Outcome::Failed {
            status: response.status.as_u16(),
        };
    }
    match business_status(&response.payload) {
        // A present envelope must be valid: a malformed status code is never
        // treated as absent and never overridden by the tool shape.
        BusinessStatus::Invalid => Outcome::Malformed,
        BusinessStatus::Valid(code) if EXHAUSTED_STATUS_CODES.contains(&code) => {
            Outcome::Exhausted { code }
        }
        BusinessStatus::Valid(code) if code != 0 => Outcome::ProviderFailed { code },
        BusinessStatus::Valid(_) => match documented_shape(tool, &response.payload) {
            true => Outcome::Success,
            false => Outcome::Malformed,
        },
        // The provider does not always repeat the envelope: a body that still
        // carries the documented success shape is accepted, anything else is a
        // malformed response instead of an empty success.
        BusinessStatus::Absent => match documented_shape(tool, &response.payload) {
            true => Outcome::Success,
            false => Outcome::Malformed,
        },
    }
}

/// Whether the payload is the documented success shape of the tool.
///
/// The shape is the one the official coding-plan MCP server documents:
/// `web_search` answers `organic` (the result array, possibly empty),
/// `understand_image` answers a non-empty `content` string.
fn documented_shape(tool: Tool, payload: &Value) -> bool {
    match tool {
        Tool::WebSearch => payload.get("organic").is_some_and(Value::is_array),
        Tool::UnderstandImage => payload
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.trim().is_empty()),
    }
}

/// The business status of a MiniMax response body.
///
/// The provider may write the code as a number or as an exact integer string;
/// any other value, a missing field inside a present envelope, and a
/// non-object envelope are malformed. `2067.0`, `null`, a list and a string
/// like `invalid` are never silently read as "absent" or `0`.
pub(super) fn business_status(payload: &Value) -> BusinessStatus {
    let Some(envelope) = payload.get("base_resp") else {
        return BusinessStatus::Absent;
    };
    let Value::Object(envelope) = envelope else {
        return BusinessStatus::Invalid;
    };
    let Some(value) = envelope.get("status_code") else {
        return BusinessStatus::Invalid;
    };
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(BusinessStatus::Valid)
            .unwrap_or(BusinessStatus::Invalid),
        Value::String(text)
            if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            text.parse::<i64>()
                .map(BusinessStatus::Valid)
                .unwrap_or(BusinessStatus::Invalid)
        }
        _ => BusinessStatus::Invalid,
    }
}
