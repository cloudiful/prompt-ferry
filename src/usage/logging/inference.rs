use crate::db::{self, RequestFailureFamily};
use crate::upstream_error::{UpstreamFailureCause, classify_upstream_failure};

use super::UsageLog;

pub(super) fn infer_failure_family(log: &UsageLog) -> Option<RequestFailureFamily> {
    if log.ok == Some(true) {
        return is_empty_success(log).then_some(RequestFailureFamily::EmptySuccess);
    }
    if log.request_state == db::RequestRecordState::Completed {
        return None;
    }
    // `log.status` is the status the client saw, which is rewritten to 429 for
    // a confirmed exhaustion. Classification needs the upstream's own status,
    // which the recorded message carries as its `HTTP <status>:` prefix.
    let status = log.status.and_then(|status| u16::try_from(status).ok());
    let upstream_status = original_upstream_status(log.error_message.as_deref()).or(status);
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

    let cause = upstream_status.and_then(|status| classify_upstream_failure(status, &haystack));
    if cause == Some(UpstreamFailureCause::Exhausted) {
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
    // A transient rate error is a rate limit; an explicitly unavailable
    // upstream is not a rate limit at all but the upstream's own 4xx/5xx.
    if cause == Some(UpstreamFailureCause::RateLimited) {
        return Some(RequestFailureFamily::RateLimit);
    }
    if cause == Some(UpstreamFailureCause::UpstreamUnavailable) {
        // Not a quota and not a rate limit: report the upstream's own class.
        return Some(match upstream_status {
            Some(status) if status >= 500 => RequestFailureFamily::Upstream5xx,
            Some(status) if (400..500).contains(&status) => RequestFailureFamily::Upstream4xx,
            _ => RequestFailureFamily::RateLimit,
        });
    }
    if upstream_status == Some(429)
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
    // Classify the upstream's own status once it is known, so a rewritten
    // client status never turns a genuine 5xx into a rate limit.
    if let Some(status) = upstream_status {
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

/// The upstream status a recorded `http_error` message preserved as its
/// `HTTP <status>:` prefix, when the message carries one.
fn original_upstream_status(error_message: Option<&str>) -> Option<u16> {
    let message = error_message?.to_ascii_lowercase();
    let start = message.find("http ")? + "http ".len();
    let digits: String = message[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() == 3 {
        digits.parse().ok()
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::infer_failure_family;
    use crate::{
        db::RequestFailureFamily,
        usage::logging::{UsageLog, UsageRequestMetadata},
    };

    fn failed(status: i32, message: &str, body: Option<&str>) -> UsageLog {
        UsageLog::ai_request(
            uuid::Uuid::new_v4(),
            UsageRequestMetadata::default(),
            Some("gpt-5".to_string()),
        )
        .with_status(Some(status), Some(false), None, None)
        .with_error(
            Some("http_error".to_string()),
            Some(message.to_string()),
            body.map(str::to_string),
        )
    }

    #[test]
    fn a_transient_429_is_a_rate_limit_and_never_a_quota() {
        for body in [
            r#"{"error":{"message":"Too Many Requests"}}"#,
            r#"{"error":{"code":2062,"message":"request rate limit"}}"#,
            "in-flight credits reserved",
            "pending requests in queue",
        ] {
            assert_eq!(
                infer_failure_family(&failed(429, "HTTP 429: upstream rate limited", Some(body))),
                Some(RequestFailureFamily::RateLimit),
                "{body}"
            );
        }
        // A bare quota/credit/billing word is no longer a quota family.
        assert_eq!(
            infer_failure_family(&failed(
                429,
                "HTTP 429: upstream rate limited",
                Some("quota")
            )),
            Some(RequestFailureFamily::RateLimit)
        );
    }

    #[test]
    fn an_explicit_unavailability_keeps_the_upstream_status_class() {
        assert_eq!(
            infer_failure_family(&failed(
                503,
                "HTTP 503: upstream server error",
                Some("Service Unavailable")
            )),
            Some(RequestFailureFamily::Upstream5xx)
        );
        assert_eq!(
            infer_failure_family(&failed(
                400,
                "HTTP 400: upstream rejected the request",
                Some("endpoint unavailable")
            )),
            Some(RequestFailureFamily::Upstream4xx)
        );
    }

    #[test]
    fn the_original_upstream_status_wins_over_the_rewritten_client_status() {
        // The client saw 429 because a real exhaustion was rewritten; the
        // stored message still names the upstream's own status.
        let exhaustion = failed(
            429,
            "HTTP 500: upstream server error; body: {}",
            Some(r#"{"error":{"code":2056,"message":"usage limit reached"}}"#),
        );
        assert_eq!(
            infer_failure_family(&exhaustion),
            Some(RequestFailureFamily::Quota)
        );
        // The same rewrite without exhaustion evidence stays a rate limit, and
        // a plain 500 without any of it stays a 5xx.
        let throttled = failed(
            429,
            "HTTP 500: upstream server error",
            Some("temporary provider failure"),
        );
        assert_eq!(
            infer_failure_family(&throttled),
            Some(RequestFailureFamily::Upstream5xx)
        );
        assert_eq!(
            infer_failure_family(&failed(500, "HTTP 500: upstream server error", None)),
            Some(RequestFailureFamily::Upstream5xx)
        );
    }
}
