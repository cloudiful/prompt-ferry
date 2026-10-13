use serde_json::{Value, json};

use crate::{
    config::NativeApi,
    db::RouteConfig,
    upstream_error::{UpstreamFailureCause, classify_upstream_failure},
    worker::runtime::{
        ai::quota_feedback::{
            AttemptFeedback, AttemptFeedbackFactory, parse_retry_after, schedule_quota_refresh,
        },
        context::RuntimeServices,
    },
};

/// Four-part handoff prompt for the self-summarize upstream call:
/// progress + decisions, constraints + preferences, remaining TODOs,
/// critical data. The model must quote exact values and never invent.
pub const SUMMARIZE_PROMPT: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that will resume the task. Include: 1) Current progress and key decisions 2) Important constraints and user preferences 3) Remaining TODOs with next steps 4) Critical data (file paths, APIs, error strings, versions). Be concise, quote exact values, do not invent.";

const SUMMARIZE_MAX_OUTPUT_TOKENS: u32 = 1024;

/// Build the upstream summarize input from pruned history: leading
/// instructions first, then the compactable history, then the handoff
/// prompt as the final user message.
pub fn build_summarize_input(head: &[Value], history: &[Value]) -> Vec<Value> {
    let mut input: Vec<Value> = Vec::with_capacity(head.len() + history.len() + 1);
    input.extend(head.iter().cloned());
    input.extend(history.iter().cloned());
    input.push(json!({
        "type": "message",
        "role": "user",
        "content": SUMMARIZE_PROMPT,
    }));
    input
}

fn summarize_responses_body(input: Vec<Value>, model: &str) -> Value {
    json!({
        "model": model,
        "input": input,
        "store": false,
        "max_output_tokens": SUMMARIZE_MAX_OUTPUT_TOKENS,
    })
}

fn history_to_chat_text(history: &[Value]) -> String {
    history
        .iter()
        .map(|item| {
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .or_else(|| item.get("type").and_then(Value::as_str))
                .unwrap_or("message");
            let text = crate::openai_compat::extract_text(item);
            format!("[{role}] {text}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Tail user messages within `keep_tail_tokens` (chars/4 estimate) for the
/// compaction `output`: the most recent user turns carry forward verbatim
/// ahead of the assistant handoff summary.
pub fn tail_user_messages(history: &[Value], keep_tail_tokens: usize) -> Vec<Value> {
    let mut tail = Vec::new();
    let mut budget = keep_tail_tokens.max(1);
    for item in history.iter().rev() {
        let is_user = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message")
            == "message"
            && item.get("role").and_then(Value::as_str) == Some("user");
        if !is_user {
            continue;
        }
        let cost = item.to_string().len().div_ceil(4).max(1);
        if cost > budget {
            break;
        }
        budget -= cost;
        tail.push(item.clone());
    }
    tail.reverse();
    tail
}

/// Extract the assistant summary text from a `/v1/responses` JSON body.
pub fn extract_summary_text_from_responses_body(body: &[u8]) -> anyhow::Result<String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| anyhow::anyhow!("summarize response is not JSON: {err}"))?;
    let text = value
        .get("output")
        .and_then(Value::as_array)
        .map(|output| crate::openai_compat::extract_text(&Value::Array(output.clone())))
        .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("summarize response carried no output text");
    }
    Ok(text)
}

/// Extract the assistant summary text from a `/v1/chat/completions` body.
pub fn extract_summary_text_from_chat_body(body: &[u8]) -> anyhow::Result<String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| anyhow::anyhow!("summarize response is not JSON: {err}"))?;
    let text = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .map(crate::openai_compat::extract_text)
        .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("summarize response carried no choice text");
    }
    Ok(text)
}

/// Assemble the `response.compaction` body: plaintext handoff only, never
/// any `encrypted_content`. Tail user messages (already within the token
/// window) carry forward verbatim; the summary becomes the final assistant
/// message so `output` replays as the next `/v1/responses` input.
pub fn assemble_compaction_response(tail: &[Value], summary: &str, model: &str) -> Value {
    let mut output: Vec<Value> = tail.to_vec();
    output.push(json!({
        "type": "message",
        "role": "assistant",
        "content": summary,
    }));
    json!({
        "id": format!("resp_compact_{}", uuid::Uuid::new_v4().simple()),
        "object": "response.compaction",
        "created_at": chrono::Utc::now().timestamp(),
        "model": model,
        "output": output,
    })
}

/// Run one same-route summarize call against the upstream and return the
/// plaintext summary text. Chat-native targets use `/v1/chat/completions`;
/// Anthropic targets use `/v1/messages`; Responses targets never reach here
/// (they passthrough natively). Never sends `context_management` and never
/// posts to `/v1/responses/compact`, so compact never recurses.
pub(super) async fn self_compact_via_upstream(
    client: &reqwest::Client,
    summarize_url: &str,
    api_key: &str,
    native_api: NativeApi,
    head: &[Value],
    history: &[Value],
    model: &str,
    services: &RuntimeServices,
    route: &RouteConfig,
    feedback_factory: &AttemptFeedbackFactory,
) -> anyhow::Result<(String, AttemptFeedback)> {
    let input = build_summarize_input(head, history);
    let (body, is_chat) = match native_api {
        NativeApi::Chat => (
            json!({
                "model": model,
                "messages": [
                    {"role": "user", "content": format!("{}\n\n{}", history_to_chat_text(&input), SUMMARIZE_PROMPT)},
                ],
                "store": false,
                "max_tokens": SUMMARIZE_MAX_OUTPUT_TOKENS,
            }),
            true,
        ),
        NativeApi::AnthropicMessages => (
            json!({
                "model": model,
                "messages": [
                    {"role": "user", "content": format!("{}\n\n{}", history_to_chat_text(&input), SUMMARIZE_PROMPT)},
                ],
                "max_tokens": SUMMARIZE_MAX_OUTPUT_TOKENS,
            }),
            false,
        ),
        _ => (summarize_responses_body(input, model), false),
    };
    let mut request = client
        .post(summarize_url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(120));
    request = match native_api {
        NativeApi::AnthropicMessages => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(api_key),
    };
    let attempt_feedback = feedback_factory.begin_attempt();
    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            attempt_feedback.release_probe();
            return Err(anyhow::anyhow!("summarize upstream request failed: {err}"));
        }
    };
    let status = response.status();
    let status_is_429 = status == http::StatusCode::TOO_MANY_REQUESTS;
    // Retained before the body is consumed for classification.
    let retry_after = parse_retry_after(response.headers());
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            if status_is_429 {
                schedule_quota_refresh(services, route);
                // A 429 whose body could not be read still backs the route off,
                // as a generic rate limit rather than a lost protection.
                attempt_feedback.record_429(retry_after);
            } else {
                attempt_feedback.release_probe();
            }
            return Err(anyhow::anyhow!("summarize upstream body failed: {err}"));
        }
    };
    let cause = classify_upstream_failure(status.as_u16(), &String::from_utf8_lossy(&bytes));
    let quota_exhausted = cause == Some(UpstreamFailureCause::Exhausted);
    if status_is_429 || quota_exhausted {
        let cause = cause.unwrap_or(UpstreamFailureCause::RateLimited);
        attempt_feedback.record_failure(if status_is_429 { retry_after } else { None }, cause);
    }
    if quota_exhausted && let Some(state) = services.admin_state() {
        state.token_plan_quota.invalidate(route.route_id).await;
    }
    if status_is_429 || quota_exhausted {
        schedule_quota_refresh(services, route);
    }
    if !status.is_success() {
        attempt_feedback.release_probe();
        anyhow::bail!("summarize upstream returned {}", status.as_u16());
    }
    let summary = if is_chat {
        extract_summary_text_from_chat_body(&bytes)
    } else if native_api == NativeApi::AnthropicMessages {
        extract_anthropic_summary_text(&bytes)
    } else {
        extract_summary_text_from_responses_body(&bytes)
    };
    match summary {
        Ok(summary) => Ok((summary, attempt_feedback)),
        Err(err) => {
            attempt_feedback.release_probe();
            Err(err)
        }
    }
}

fn extract_anthropic_summary_text(body: &[u8]) -> anyhow::Result<String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| anyhow::anyhow!("summarize response is not JSON: {err}"))?;
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        anyhow::bail!("summarize response carried no content text");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::{
        SUMMARIZE_PROMPT, assemble_compaction_response, build_summarize_input,
        extract_summary_text_from_chat_body, extract_summary_text_from_responses_body,
        tail_user_messages,
    };
    use serde_json::json;

    #[test]
    fn prompt_covers_all_four_handoff_parts() {
        for part in ["Current progress", "constraints", "TODO", "Critical data"] {
            assert!(SUMMARIZE_PROMPT.contains(part), "missing {part}");
        }
        assert!(SUMMARIZE_PROMPT.contains("do not invent"));
    }

    #[test]
    fn summarize_input_appends_handoff_prompt_last() {
        let head = vec![json!({"type": "message", "role": "system", "content": "sys"})];
        let history = vec![json!({"type": "message", "role": "user", "content": "hi"})];
        let input = build_summarize_input(&head, &history);
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["content"], "sys");
        assert!(input[2]["content"].as_str().unwrap().contains("COMPACTION"));
    }

    #[test]
    fn extracts_responses_output_text() {
        let body = br#"{"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"  handoff  "}]}]}"#;
        assert_eq!(
            extract_summary_text_from_responses_body(body).unwrap(),
            "handoff"
        );
    }

    #[test]
    fn extracts_chat_choice_text() {
        let body = br#"{"choices":[{"message":{"role":"assistant","content":"summary here"}}]}"#;
        assert_eq!(
            extract_summary_text_from_chat_body(body).unwrap(),
            "summary here"
        );
    }

    #[test]
    fn tail_user_messages_keeps_recent_user_turns_only() {
        let history = vec![
            json!({"type": "message", "role": "user", "content": "old"}),
            json!({"type": "function_call", "call_id": "c1", "name": "read", "arguments": "{}"}),
            json!({"type": "message", "role": "user", "content": "new"}),
        ];
        let tail = tail_user_messages(&history, 15_000);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0]["content"], "old");
        assert_eq!(tail[1]["content"], "new");
        let tiny = tail_user_messages(&history, 1);
        assert!(tiny.len() <= 1);
    }

    #[test]
    fn compaction_response_has_no_encrypted_content() {
        let tail = vec![json!({"type": "message", "role": "user", "content": "q"})];
        let response = assemble_compaction_response(&tail, "done: x", "m");
        assert_eq!(response["object"], "response.compaction");
        assert!(
            response["id"]
                .as_str()
                .unwrap()
                .starts_with("resp_compact_")
        );
        assert_eq!(response["output"].as_array().unwrap().len(), 2);
        assert_eq!(response["output"][1]["role"], "assistant");
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains("encrypted_content"));
    }
}
