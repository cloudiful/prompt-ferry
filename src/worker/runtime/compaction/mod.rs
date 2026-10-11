use serde_json::Value;

use crate::{
    config::NativeApi,
    db::RouteConfig,
    worker::runtime::{
        ai::quota_feedback::{AttemptFeedback, AttemptFeedbackFactory},
        context::RuntimeServices,
    },
};

pub mod prune;
#[cfg(test)]
#[path = "quota_feedback_tests.rs"]
mod quota_feedback_tests;
pub mod summarize;

pub use prune::prune_for_compact;
pub use summarize::{assemble_compaction_response, tail_user_messages};

const COMPACT_TAIL_TOKENS: usize = 15_000;

/// Run the ferry-side self-summarize flow for one compact request body:
/// parse `input`/`model`, prune (drop all `encrypted_content`, trim tool
/// history), summarize via the same-route upstream, and assemble the
/// plaintext `response.compaction` body. Returns `(body, summary_text)`.
/// Never fabricates `encrypted_content` and never calls compact recursively.
pub(super) async fn run_self_summarize_compact(
    client: &reqwest::Client,
    summarize_url: &str,
    api_key: &str,
    native_api: NativeApi,
    request_body: &[u8],
    model_fallback: Option<&str>,
    services: &RuntimeServices,
    route: &RouteConfig,
    feedback_factory: &AttemptFeedbackFactory,
) -> anyhow::Result<(Vec<u8>, String, AttemptFeedback)> {
    let value: Value = serde_json::from_slice(request_body)
        .map_err(|err| anyhow::anyhow!("compact body must be JSON object: {err}"))?;
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            model_fallback
                .map(str::trim)
                .filter(|model| !model.is_empty())
        })
        .unwrap_or("unknown");
    let items: Vec<Value> = match value.get("input") {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => vec![serde_json::json!({
            "type": "message",
            "role": "user",
            "content": text,
        })],
        _ => anyhow::bail!("compact.input must be array or string"),
    };
    let (head, history) = prune_for_compact(items, COMPACT_TAIL_TOKENS);
    let (summary, feedback) = summarize::self_compact_via_upstream(
        client,
        summarize_url,
        api_key,
        native_api,
        &head,
        &history,
        model,
        services,
        route,
        feedback_factory,
    )
    .await?;
    let tail = tail_user_messages(&history, COMPACT_TAIL_TOKENS);
    let response = assemble_compaction_response(&tail, &summary, model);
    let body = serde_json::to_vec(&response)?;
    Ok((body, summary, feedback))
}
