use reqwest::Client;
use serde_json::Value;

use crate::usage::truncate_chars;

use super::{LlmReviewSettings, ReviewFailure, ReviewResult, parse_review_completion_body};

pub const LLM_REVIEW_SETTINGS_KEY: &str = "llm_review_settings";
const REVIEW_SYSTEM_PROMPT: &str = "You are a request safety reviewer for an OpenAI-compatible proxy.\nReturn JSON only.\nDecide whether the request can pass upstream immediately or must be held for human approval.\nAllowed decisions: allow, flag.\nRequired JSON shape: {\"decision\":\"allow|flag\",\"reason\":\"short reason\",\"categories\":[\"category\"]}.\nDo not return markdown. Do not return extra text.";

#[derive(Debug, Clone)]
pub struct ReviewRequest<'a> {
    pub path: &'a str,
    pub model: Option<&'a str>,
    pub request_preview: &'a str,
    pub request_payload_json: &'a Value,
}

pub async fn review_request(
    client: &Client,
    settings: &LlmReviewSettings,
    request: ReviewRequest<'_>,
) -> Result<ReviewResult, ReviewFailure> {
    let prompt = build_review_prompt(settings, &request);
    let response = client
        .post(upstream_url(
            &settings.review_base_url,
            "/v1/chat/completions",
        ))
        .timeout(std::time::Duration::from_millis(settings.review_timeout_ms))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({
            "model": settings.review_model,
            "stream": false,
            "messages": [
                { "role": "system", "content": REVIEW_SYSTEM_PROMPT },
                { "role": "user", "content": prompt },
            ],
        }));
    let response = if settings.review_api_key.trim().is_empty() {
        response
    } else {
        response.bearer_auth(&settings.review_api_key)
    };

    let response = response.send().await.map_err(|err| {
        if err.is_timeout() {
            ReviewFailure::Timeout
        } else {
            ReviewFailure::Error(err.to_string())
        }
    })?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| ReviewFailure::Error(err.to_string()))?;
    if !status.is_success() {
        return Err(ReviewFailure::Error(format!(
            "review endpoint returned HTTP {}: {}",
            status.as_u16(),
            truncate_chars(&String::from_utf8_lossy(&body), 400),
        )));
    }
    parse_review_completion_body(&body).map_err(|err| ReviewFailure::Error(err.to_string()))
}

fn build_review_prompt(settings: &LlmReviewSettings, request: &ReviewRequest<'_>) -> String {
    let mut prompt = String::new();
    prompt.push_str("Review this proxied request.\n");
    prompt.push_str("Return JSON only.\n");
    prompt.push_str(&format!("Path: {}\n", request.path));
    prompt.push_str(&format!(
        "Model: {}\n",
        request.model.unwrap_or("(missing)")
    ));
    if !settings.custom_policy_text.trim().is_empty() {
        prompt.push_str("\nCustom policy:\n");
        prompt.push_str(settings.custom_policy_text.trim());
        prompt.push('\n');
    }
    prompt.push_str("\nNormalized preview:\n");
    prompt.push_str(if request.request_preview.trim().is_empty() {
        "(empty)"
    } else {
        request.request_preview.trim()
    });
    prompt.push_str("\n\nRequest JSON:\n");
    let payload_text = serde_json::to_string_pretty(request.request_payload_json)
        .unwrap_or_else(|_| request.request_payload_json.to_string());
    prompt.push_str(&truncate_chars(&payload_text, 16_000));
    prompt
}

pub fn request_payload_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or_else(|_| {
        serde_json::json!({
            "_raw_body": truncate_chars(&String::from_utf8_lossy(body), 16_000)
        })
    })
}

pub fn compute_wait_deadline_unix_ms(
    now_unix_ms: i64,
    request_deadline_unix_ms: i64,
    review_timeout_ms: u64,
    pending_ttl_seconds: u64,
) -> i64 {
    let ttl_deadline =
        now_unix_ms.saturating_add((pending_ttl_seconds as i64).saturating_mul(1_000));
    let relay_safe_deadline = request_deadline_unix_ms
        .saturating_sub(review_timeout_ms as i64)
        .saturating_sub(5_000);
    ttl_deadline.min(relay_safe_deadline)
}

fn upstream_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_wait_deadline_before_relay_timeout() {
        let deadline = compute_wait_deadline_unix_ms(10_000, 310_000, 3_000, 300);
        assert_eq!(deadline, 302_000);
    }
}
