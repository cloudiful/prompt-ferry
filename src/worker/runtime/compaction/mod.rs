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

/// The inputs of one ferry-side self-summarize compact call.
pub(super) struct SelfSummarizeInput<'a> {
    pub(super) client: &'a reqwest::Client,
    pub(super) summarize_url: &'a str,
    pub(super) api_key: &'a str,
    pub(super) native_api: NativeApi,
    pub(super) request_body: &'a [u8],
    pub(super) model_fallback: Option<&'a str>,
    pub(super) services: &'a RuntimeServices,
    pub(super) route: &'a RouteConfig,
    pub(super) feedback_factory: &'a AttemptFeedbackFactory,
}

/// Run the ferry-side self-summarize flow for one compact request body:
/// parse `input`/`model`, prune (drop all `encrypted_content`, trim tool
/// history), summarize via the same-route upstream, and assemble the
/// plaintext `response.compaction` body. Returns `(body, summary_text)`.
/// Never fabricates `encrypted_content` and never calls compact recursively.
pub(super) async fn run_self_summarize_compact(
    input: SelfSummarizeInput<'_>,
) -> anyhow::Result<(Vec<u8>, String, AttemptFeedback)> {
    let SelfSummarizeInput {
        client,
        summarize_url,
        api_key,
        native_api,
        request_body,
        model_fallback,
        services,
        route,
        feedback_factory,
    } = input;
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
    let (summary, feedback) = summarize::self_compact_via_upstream(summarize::SummarizeCall {
        client,
        summarize_url,
        api_key,
        native_api,
        head: &head,
        history: &history,
        model,
        services,
        route,
        feedback_factory,
    })
    .await?;
    let tail = tail_user_messages(&history, COMPACT_TAIL_TOKENS);
    let response = assemble_compaction_response(&tail, &summary, model);
    let body = serde_json::to_vec(&response)?;
    Ok((body, summary, feedback))
}
