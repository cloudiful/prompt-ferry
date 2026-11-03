//! The per-request MCP call tracker (issue #1179 P2).
//!
//! One tracker belongs to one inbound MCP request. The request handler, the
//! rmcp session task that serves the request and the settlement that records
//! the request share it, so the selected endpoint key and the actual tool
//! outcome survive the task boundary. Two requests never share a tracker, so
//! two sessions that use the same JSON-RPC id cannot leak a key or an outcome
//! into each other.
//!
//! The tracker records the outcome of the call, never the payloads: it holds
//! an endpoint key id, a key label, a token slot, a stable error code and a
//! bounded message.

use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

/// The stable internal code of a tool result that reported a failure.
pub(crate) const TOOL_ERROR_CODE: &str = "mcp_tool_error";
/// The stable internal code of an upstream JSON-RPC error.
pub(crate) const UPSTREAM_ERROR_CODE: &str = "mcp_upstream_error";
/// The stable internal code of a call that never produced a result.
pub(crate) const CALL_ERROR_CODE: &str = "mcp_call_failed";
/// The stable internal code of a provider-reported spent plan quota.
pub(crate) const QUOTA_EXHAUSTED_CODE: &str = "mcp_quota_exhausted";

/// Why a tracked tool call failed.
///
/// The tracker turns one of these into a fixed diagnostic: it never stores the
/// provider's error text, a transport exception, a query or an image payload.
/// An upstream that echoes the bearer token or the request input in its error
/// therefore cannot reach the request record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum McpCallFailure {
    /// The tool result reported `isError`.
    ToolError,
    /// The upstream answered with a JSON-RPC error.
    UpstreamError { code: Option<i64> },
    /// The call never produced a result.
    CallFailed { status: Option<u16> },
    /// The provider reported a spent plan quota.
    QuotaExhausted { provider_code: i64 },
}

impl McpCallFailure {
    /// The stable code and the safe, fixed message of this failure.
    fn diagnostic(self) -> (&'static str, String) {
        match self {
            Self::ToolError => (
                TOOL_ERROR_CODE,
                "the tool reported an error result".to_string(),
            ),
            Self::UpstreamError { code: Some(code) } => (
                UPSTREAM_ERROR_CODE,
                format!("the upstream returned JSON-RPC error {code}"),
            ),
            Self::UpstreamError { code: None } => (
                UPSTREAM_ERROR_CODE,
                "the upstream returned a JSON-RPC error".to_string(),
            ),
            Self::CallFailed {
                status: Some(status),
            } => (
                CALL_ERROR_CODE,
                format!("the upstream call failed with HTTP {status}"),
            ),
            Self::CallFailed { status: None } => {
                (CALL_ERROR_CODE, "the upstream call failed".to_string())
            }
            Self::QuotaExhausted { provider_code } => (
                QUOTA_EXHAUSTED_CODE,
                format!("the provider reported an exhausted plan quota ({provider_code})"),
            ),
        }
    }
}

/// The outcome of the tracked tool call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum McpCallOutcome {
    /// No tool call was observed: the request never reached one.
    #[default]
    NotObserved,
    /// The tool call completed.
    Completed,
    /// The tool call failed.
    Failed { code: String, message: String },
}

/// The recorded state of one tracked call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct McpCallRecord {
    /// The endpoint key that served the last attempt.
    pub endpoint_key_id: Option<Uuid>,
    /// The label of that endpoint key.
    pub endpoint_key_label: Option<String>,
    /// The one-based MCP token slot of the last attempt.
    pub mcp_bearer_token_slot: Option<i16>,
    /// The outcome of the tracked call.
    pub outcome: McpCallOutcome,
    /// How many attempts failed before the call settled.
    pub attempt_failures: usize,
}

impl McpCallRecord {
    /// The recorded failure, when the call failed.
    pub(crate) fn failure(&self) -> Option<(&str, &str)> {
        match &self.outcome {
            McpCallOutcome::Failed { code, message } => Some((code.as_str(), message.as_str())),
            _ => None,
        }
    }
}

/// The shared tracker of one inbound request.
#[derive(Clone, Default)]
pub(crate) struct McpCallTracker(Arc<Mutex<TrackedState>>);

#[derive(Default)]
struct TrackedState {
    endpoint_key_id: Option<Uuid>,
    endpoint_key_label: Option<String>,
    mcp_bearer_token_slot: Option<i16>,
    completed: bool,
    failure: Option<(String, String)>,
    attempt_failures: usize,
}

impl McpCallTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record the endpoint key that served the last attempt.
    pub(crate) fn record_endpoint_key(&self, key_id: Option<Uuid>, label: Option<String>) {
        let mut state = self.lock();
        state.endpoint_key_id = key_id;
        state.endpoint_key_label = label;
    }

    /// Record the one-based MCP token slot of the last attempt.
    pub(crate) fn record_token_slot(&self, slot: Option<i16>) {
        let mut state = self.lock();
        state.mcp_bearer_token_slot = slot;
    }

    /// Record an attempt that failed while another key may still serve the
    /// call. It never settles the call on its own.
    pub(crate) fn record_attempt_failure(&self) {
        let mut state = self.lock();
        state.attempt_failures = state.attempt_failures.saturating_add(1);
    }

    /// Record that the tracked call completed.
    ///
    /// A success never clears a recorded failure: a later unrelated success in
    /// the same request must not hide an error that already happened.
    pub(crate) fn record_success(&self) {
        let mut state = self.lock();
        state.completed = true;
    }

    /// Record that the tracked call failed. The first failure wins, so the
    /// most precise cause is kept.
    ///
    /// Only a fixed diagnostic derived from the failure kind is stored: the
    /// upstream text, the transport message and the payload never reach the
    /// record, whatever the content logging mode is.
    pub(crate) fn record_failure(&self, failure: McpCallFailure) {
        let mut state = self.lock();
        if state.failure.is_none() {
            let (code, message) = failure.diagnostic();
            state.failure = Some((code.to_string(), message));
        }
    }

    /// The recorded state of the call.
    pub(crate) fn snapshot(&self) -> McpCallRecord {
        let state = self.lock();
        let outcome = match &state.failure {
            Some((code, message)) => McpCallOutcome::Failed {
                code: code.clone(),
                message: message.clone(),
            },
            None if state.completed => McpCallOutcome::Completed,
            None => McpCallOutcome::NotObserved,
        };
        McpCallRecord {
            endpoint_key_id: state.endpoint_key_id,
            endpoint_key_label: state.endpoint_key_label.clone(),
            mcp_bearer_token_slot: state.mcp_bearer_token_slot,
            outcome,
            attempt_failures: state.attempt_failures,
        }
    }

    fn lock(&self) -> MutexGuard<'_, TrackedState> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Record the outcome of a JSON-RPC response.
///
/// A JSON-RPC error and a tool result that reports `isError` are both tool
/// failures, even when the transport answered with HTTP 200.
pub(crate) fn record_response_outcome(tracker: &McpCallTracker, response: &serde_json::Value) {
    if let Some(error) = response.get("error") {
        // Only the numeric code is safe: the message may repeat the bearer
        // token, the query or an image payload.
        let code = error.get("code").and_then(serde_json::Value::as_i64);
        tracker.record_failure(McpCallFailure::UpstreamError { code });
        return;
    }
    if response
        .pointer("/result/isError")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        tracker.record_failure(McpCallFailure::ToolError);
        return;
    }
    tracker.record_success();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> McpCallTracker {
        McpCallTracker::new()
    }

    #[test]
    fn a_fallback_success_overrides_the_intermediate_attempt_failures() {
        let tracker = tracker();
        tracker.record_attempt_failure();
        tracker.record_attempt_failure();
        tracker.record_success();

        let record = tracker.snapshot();
        assert_eq!(record.outcome, McpCallOutcome::Completed);
        assert_eq!(record.attempt_failures, 2);
    }

    #[test]
    fn a_later_success_never_overwrites_a_recorded_failure() {
        let tracker = tracker();
        tracker.record_failure(McpCallFailure::ToolError);
        tracker.record_success();

        let record = tracker.snapshot();
        assert_eq!(
            record.outcome,
            McpCallOutcome::Failed {
                code: TOOL_ERROR_CODE.to_string(),
                message: "the tool reported an error result".to_string(),
            }
        );
    }

    #[test]
    fn the_first_failure_wins() {
        let tracker = tracker();
        tracker.record_failure(McpCallFailure::ToolError);
        tracker.record_failure(McpCallFailure::CallFailed { status: Some(500) });

        let record = tracker.snapshot();
        assert_eq!(
            record.failure(),
            Some((TOOL_ERROR_CODE, "the tool reported an error result"))
        );
    }

    #[test]
    fn a_request_without_a_tool_call_observes_no_outcome() {
        let tracker = tracker();
        assert_eq!(tracker.snapshot().outcome, McpCallOutcome::NotObserved);
    }

    #[test]
    fn a_tool_error_result_is_a_failure() {
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({"result": {"content": [], "isError": true}}),
        );

        assert_eq!(
            tracker.snapshot().failure().map(|(code, _)| code),
            Some(TOOL_ERROR_CODE)
        );
    }

    #[test]
    fn a_json_rpc_error_keeps_only_its_numeric_code() {
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({"error": {"code": -32001, "message": "tool failed"}}),
        );

        let record = tracker.snapshot();
        assert_eq!(
            record.failure().map(|(code, _)| code),
            Some(UPSTREAM_ERROR_CODE)
        );
        assert_eq!(
            record.failure().map(|(_, message)| message),
            Some("the upstream returned JSON-RPC error -32001")
        );
    }

    #[test]
    fn a_tool_error_result_keeps_a_fixed_message_whatever_it_carries() {
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({
                "result": {
                    "content": [{ "type": "text", "text": "sk-secret failed for query rust" }],
                    "isError": true
                }
            }),
        );

        assert_eq!(
            tracker.snapshot().failure(),
            Some((TOOL_ERROR_CODE, "the tool reported an error result"))
        );
    }

    #[test]
    fn a_json_rpc_error_without_a_numeric_code_keeps_a_fixed_message() {
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({"error": {"code": "boom", "message": "tool failed"}}),
        );

        assert_eq!(
            tracker.snapshot().failure().map(|(_, message)| message),
            Some("the upstream returned a JSON-RPC error")
        );
    }

    #[test]
    fn a_provider_message_never_reaches_the_record() {
        // Issue #1179 P2 repair: the provider echoes the bearer token and the
        // request input in its error message.
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({
                "error": {
                    "code": -32001,
                    "message": "request for sk-secret with query \"rust\" failed (2056)"
                }
            }),
        );

        let record = tracker.snapshot();
        let (_, message) = record.failure().expect("a failure");
        assert!(!message.contains("sk-secret"), "{message}");
        assert!(!message.contains("rust"), "{message}");
        assert!(!message.contains("2056"), "{message}");
        assert_eq!(message, "the upstream returned JSON-RPC error -32001");
    }

    #[test]
    fn every_failure_kind_carries_a_fixed_safe_message() {
        for (failure, code, message) in [
            (
                McpCallFailure::ToolError,
                TOOL_ERROR_CODE,
                "the tool reported an error result",
            ),
            (
                McpCallFailure::UpstreamError { code: Some(-32603) },
                UPSTREAM_ERROR_CODE,
                "the upstream returned JSON-RPC error -32603",
            ),
            (
                McpCallFailure::CallFailed { status: Some(502) },
                CALL_ERROR_CODE,
                "the upstream call failed with HTTP 502",
            ),
            (
                McpCallFailure::CallFailed { status: None },
                CALL_ERROR_CODE,
                "the upstream call failed",
            ),
            (
                McpCallFailure::QuotaExhausted {
                    provider_code: 2056,
                },
                QUOTA_EXHAUSTED_CODE,
                "the provider reported an exhausted plan quota (2056)",
            ),
        ] {
            let tracker = tracker();
            tracker.record_failure(failure);
            assert_eq!(tracker.snapshot().failure(), Some((code, message)));
        }
    }

    #[test]
    fn a_completed_result_is_a_success() {
        let tracker = tracker();
        record_response_outcome(
            &tracker,
            &serde_json::json!({"result": {"content": [{"type": "text", "text": "hi"}]}}),
        );

        assert_eq!(tracker.snapshot().outcome, McpCallOutcome::Completed);
    }

    #[test]
    fn the_last_attempt_wins_for_the_key_and_the_slot() {
        let tracker = tracker();
        let first = Uuid::new_v4();
        let last = Uuid::new_v4();
        tracker.record_endpoint_key(Some(first), Some("first".to_string()));
        tracker.record_token_slot(Some(1));
        tracker.record_endpoint_key(Some(last), Some("last".to_string()));
        tracker.record_token_slot(Some(2));

        let record = tracker.snapshot();
        assert_eq!(record.endpoint_key_id, Some(last));
        assert_eq!(record.endpoint_key_label.as_deref(), Some("last"));
        assert_eq!(record.mcp_bearer_token_slot, Some(2));
    }
}
