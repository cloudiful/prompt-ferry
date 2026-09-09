//! GLM runtime envelope check (issue #241).
//!
//! The Zhipu Coding Plan returns HTTP 200 with a JSON envelope
//! `{code, msg, success}` even when the underlying request failed
//! (e.g. a non-existent route returns
//! `{"code":500,"msg":"404 NOT_FOUND","success":false}`). The runtime
//! HTTP path used to treat any 2xx as success, which means a dead
//! route was recorded as an empty success instead of surfacing the
//! envelope error. The Chat and Responses non-stream forwarders
//! consumed the envelope body as a normal payload and either
//! produced a malformed translated response or forwarded the
//! envelope bytes to the client.
//!
//! Standard OpenAI-shape bodies (`{id, object, choices, ...}` for
//! Chat, `{id, object, output, ...}` for Responses) never carry the
//! `success`/`code` envelope, so the check is non-invasive for the
//! happy path. The 0/200 `code`-as-success convention matches the
//! `worker_admin::glm_parsing::glm_envelope_error` helper; the
//! runtime variant is a small standalone function so the
//! `worker_admin` module boundary stays intact.
//!
//! Scope: GLM provider, Chat and Responses native APIs only. GLM's
//! Anthropic arm is a native Anthropic bridge (no Zhipu envelope).
//! The Zhipu envelope is only ever delivered as a non-SSE JSON body
//! (the dead-route failure is a 200 `application/json` response even
//! when the client asked for a stream), so the live forward paths
//! that consume a 2xx JSON body are guarded:
//!
//! - The Passthrough + non-SSE + JSON path is handled centrally in
//!   `forward_upstream_response` via [`envelope_preflight_eligible`]
//!   + [`consume_envelope_preflight`] (issue #241). Without it the
//!   streaming/buffered entry would receive the envelope bytes and
//!   emit them as a 200 success.
//! - The translation branches (ChatToResponses / ResponsesToChat /
//!   Responses passthrough non-stream) run the check on the body they
//!   already read, because those paths must not double-buffer the
//!   response through the central preflight.
//!
//! SSE responses are streamed chunk-by-chunk and cannot be peeked,
//! but they are out of scope: a genuine Zhipu SSE stream never
//! carries the `{code, msg, success}` envelope shape.

use http::StatusCode;
use serde_json::Value;

use crate::{
    config::NativeApi, db::EndpointProvider, openai_compat::CompatError,
    upstream_adapter::ResponseAdapter,
};

/// Return `Some(CompatError)` when the configured upstream response
/// body is a GLM business-envelope failure. `None` means the body is
/// either a normal OpenAI-shape payload (no envelope fields) or the
/// call is not GLM-scoped.
pub(in crate::worker::runtime) fn check_glm_envelope_error(
    body: &[u8],
    provider: EndpointProvider,
    native_api: NativeApi,
) -> Option<CompatError> {
    if provider != EndpointProvider::Glm {
        return None;
    }
    if !matches!(native_api, NativeApi::Chat | NativeApi::Responses) {
        return None;
    }
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let (code, message) = glm_envelope_code_and_message(&value)?;
    let display = match code {
        Some(code) => format!("GLM rejected the request ({code}): {message}"),
        None => format!("GLM rejected the request: {message}"),
    };
    Some(CompatError::new(
        StatusCode::BAD_GATEWAY,
        "glm_envelope_error",
        display,
    ))
}

/// True when the centralized envelope preflight in
/// `forward_upstream_response` should buffer and inspect the body
/// before the existing routing branches run. Covers Passthrough +
/// non-SSE + GLM Chat/Responses + JSON content-type; translation
/// branches (ChatToResponses / ResponsesToChat) and the Anthropic
/// arm do their own per-forwarder check.
pub(in crate::worker::runtime) fn envelope_preflight_eligible(
    provider: EndpointProvider,
    native_api: NativeApi,
    content_type: Option<&str>,
    is_sse: bool,
    response_adapter: ResponseAdapter,
) -> bool {
    provider == EndpointProvider::Glm
        && matches!(native_api, NativeApi::Chat | NativeApi::Responses)
        && !is_sse
        && content_type.is_some_and(|value| value.contains("application/json"))
        && matches!(response_adapter, ResponseAdapter::Passthrough)
}

/// Buffered envelope preflight outcome. The Passthrough + non-SSE +
/// GLM + JSON path consumes the response body here so the envelope
/// check runs before any downstream translation or streaming sees it.
pub(in crate::worker::runtime) enum EnvelopePreflightOutcome {
    /// No envelope detected; the rebuilt response carries the
    /// upstream's normal JSON payload so the existing routing
    /// branches can consume it via `bytes_stream()` /
    /// `read_response_limited`.
    Pass(reqwest::Response),
    /// Envelope failure detected; surface as 502 with code
    /// `glm_envelope_error`.
    Fail(CompatError),
}

/// Consume a GLM-eligible 2xx response, read its body, and decide
/// whether the envelope check trips. The caller MUST have already
/// verified [`envelope_preflight_eligible`] returned `true`. The read
/// is bounded by `max_bytes` (the same `max_upstream_response_bytes`
/// the buffered forwarders enforce) so a misbehaving upstream cannot
/// force a full oversized body into memory. The `Pass` arm rebuilds a
/// fresh `reqwest::Response` from the buffered bytes (preserving
/// status + original headers) so the existing routing branches can
/// stream or buffer the body unchanged.
pub(in crate::worker::runtime) async fn consume_envelope_preflight(
    response: reqwest::Response,
    provider: EndpointProvider,
    native_api: NativeApi,
    max_bytes: usize,
) -> anyhow::Result<EnvelopePreflightOutcome> {
    debug_assert!(provider == EndpointProvider::Glm);
    debug_assert!(matches!(native_api, NativeApi::Chat | NativeApi::Responses));
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = read_response_bounded(response, max_bytes).await?;
    if let Some(err) = check_glm_envelope_error(&bytes, provider, native_api) {
        return Ok(EnvelopePreflightOutcome::Fail(err));
    }
    let mut builder = http::Response::builder().status(status);
    // Preserve the original upstream headers (minus hop-by-hop length
    // headers, which the buffered body re-derives) so the downstream
    // branches still forward `request-id` / ratelimit headers to the
    // client exactly as before the preflight existed.
    for (name, value) in headers.iter() {
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }
    let new_response: reqwest::Response = builder
        .body(reqwest::Body::from(bytes))
        .expect("preflight response builder must accept buffered body")
        .into();
    Ok(EnvelopePreflightOutcome::Pass(new_response))
}

/// Read a response body up to `max_bytes`, mirroring the bounded read
/// used by the buffered non-stream forwarders so an oversized upstream
/// payload fails with the same `upstream_response_too_large` signal
/// instead of being buffered unconditionally.
async fn read_response_bounded(
    response: reqwest::Response,
    max_bytes: usize,
) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed reading upstream response body")?;
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|bytes| bytes > max_bytes)
        {
            return Err(anyhow::anyhow!("upstream_response_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reuse the 0/200 `code`-as-success convention from the admin
/// parser. Returns `(Option<code>, message)` for an envelope failure
/// and `None` when the body is either a normal payload or an
/// envelope success (no `success == false` flag, no non-0/200
/// `code`).
pub(super) fn glm_envelope_code_and_message(value: &Value) -> Option<(Option<String>, String)> {
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        if code == 0 || code == 200 {
            return None;
        }
        let message = value
            .get("msg")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("upstream returned a business error")
            .to_string();
        return Some((Some(code.to_string()), message));
    }
    if value.get("success").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("msg")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("upstream returned a business error")
            .to_string();
        return Some((None, message));
    }
    None
}
