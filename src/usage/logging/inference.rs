use crate::db::{self, RequestFailureFamily};
use crate::upstream_error::is_quota_exhaustion;

use super::UsageLog;

pub(super) fn infer_failure_family(log: &UsageLog) -> Option<RequestFailureFamily> {
    if log.ok == Some(true) {
        return is_empty_success(log).then_some(RequestFailureFamily::EmptySuccess);
    }
    if log.request_state == db::RequestRecordState::Completed {
        return None;
    }
    let status = log.status;
    let error_code = log
        .error_code
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let error_message = log
        .error_message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let upstream_error_body = log
        .upstream_error_body
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let haystack = format!("{error_code} {error_message} {upstream_error_body}");

    if is_quota_exhaustion(&haystack) {
        return Some(RequestFailureFamily::Quota);
    }
    if matches!(status, Some(401 | 403))
        || haystack.contains("auth")
        || haystack.contains("unauthor")
        || haystack.contains("forbidden")
        || haystack.contains("invalid_api_key")
    {
        return Some(RequestFailureFamily::Auth);
    }
    if status == Some(429)
        || haystack.contains("rate_limit")
        || haystack.contains("too many requests")
    {
        return Some(RequestFailureFamily::RateLimit);
    }
    if haystack.contains("timeout") || haystack.contains("deadline") {
        return Some(RequestFailureFamily::Timeout);
    }
    if haystack.contains("budget_exceeded")
        || haystack.contains("policy")
        || haystack.contains("approval")
    {
        return Some(RequestFailureFamily::Policy);
    }
    if let Some(status) = status {
        if (400..500).contains(&status) {
            return Some(RequestFailureFamily::Upstream4xx);
        }
        if status >= 500 {
            return Some(RequestFailureFamily::Upstream5xx);
        }
    }
    if haystack.contains("transport")
        || haystack.contains("connection")
        || haystack.contains("stream_error")
        || haystack.contains("network")
        || haystack.contains("dns")
    {
        return Some(RequestFailureFamily::Network);
    }
    (!haystack.trim().is_empty()).then_some(RequestFailureFamily::Unknown)
}

fn is_empty_success(log: &UsageLog) -> bool {
    if log.request_category != db::RequestRecordCategory::Ai {
        return false;
    }
    let no_output_tokens = log.usage.output_tokens.unwrap_or(0) == 0;
    let no_response_text = log
        .response_prompt
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty);
    no_output_tokens && no_response_text
}
