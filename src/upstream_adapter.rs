use http::StatusCode;

use crate::{
    anthropic_compat::validate_messages_request_body,
    config::NativeApi,
    openai_compat::{
        CompatError, chat_request_to_responses, normalize_chat_request_for_native,
        responses_stateless_request_to_chat, validate_raw_compact_request_body,
        validate_raw_responses_request_body,
    },
    redact_upstream::UpstreamRedactionSession,
    usage::upstream_body,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseAdapter {
    Passthrough,
    ChatToResponses,
    ResponsesToChat,
    AnthropicMessagesToResponses,
    /// Issue #502 Task 4: `/v1/responses/compact` on a non-Responses target
    /// with `compact_mode=self_summarize`. No upstream compact call is made;
    /// the worker runs the ferry-side prune + LLM handoff flow instead.
    SelfSummarizeLocal,
}

#[derive(Debug, Clone)]
pub enum PreparedRequestBody {
    PassthroughStream(Vec<u8>),
    BufferedBytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct PreparedUpstreamRequest {
    pub path: String,
    pub body: PreparedRequestBody,
    pub response_adapter: ResponseAdapter,
    pub upstream_redacted_request_json: Option<serde_json::Value>,
    pub upstream_restore_session: Option<UpstreamRedactionSession>,
}

pub fn prepare_upstream_request(
    request_path: &str,
    request_body: &[u8],
    native_api: NativeApi,
    dev_system_normalize: bool,
    thinking_effort_override: Option<&str>,
) -> Result<PreparedUpstreamRequest, CompatError> {
    prepare_upstream_request_with_compact(
        request_path,
        request_body,
        native_api,
        dev_system_normalize,
        thinking_effort_override,
        crate::db::CompactMode::Passthrough,
    )
}

/// Issue #502 Task 5: compact-mode-aware dispatch. `passthrough` (default)
/// keeps the Task 3 behavior byte-for-byte; `off` rejects compact on any
/// target; `self_summarize` lets non-Responses targets run the ferry-side
/// handoff flow instead of the cross-protocol 400.
///
/// Issue #637: this route-less entry point delegates with no provider
/// context, so a caller `service_tier` is rejected on Responses→Chat;
/// Chat→Responses forwards the caller field for every provider (baseline
/// behavior — the configuration-based override stays gated in the separate
/// live transform). Pass a provider via
/// `prepare_upstream_request_for_provider` for the provider-aware
/// Responses→Chat admission.
pub fn prepare_upstream_request_with_compact(
    request_path: &str,
    request_body: &[u8],
    native_api: NativeApi,
    dev_system_normalize: bool,
    thinking_effort_override: Option<&str>,
    compact_mode: crate::db::CompactMode,
) -> Result<PreparedUpstreamRequest, CompatError> {
    prepare_upstream_request_for_provider(
        request_path,
        request_body,
        native_api,
        None,
        dev_system_normalize,
        thinking_effort_override,
        compact_mode,
    )
}

/// Issue #637: provider-aware dispatch threading the resolved route
/// provider into the compatibility translation. On Responses→Chat the
/// caller `service_tier` is accepted and forwarded only for supported
/// providers (the shared `EndpointProvider::supports_service_tier` bit also
/// used by the live transform and the route probe); unrelated providers
/// and route-less callers keep the previous explicit `unsupported_feature`
/// rejection there. On Chat→Responses the caller field is always forwarded
/// (baseline behavior, plan decision 3: no configured value preserves the
/// caller's field) — the configuration-based override stays gated by the
/// provider/protocol matrix in the separate live transform, which never
/// injects for unsupported combinations. The live request path always
/// passes `Some`.
pub fn prepare_upstream_request_for_provider(
    request_path: &str,
    request_body: &[u8],
    native_api: NativeApi,
    provider: Option<crate::db::EndpointProvider>,
    dev_system_normalize: bool,
    thinking_effort_override: Option<&str>,
    compact_mode: crate::db::CompactMode,
) -> Result<PreparedUpstreamRequest, CompatError> {
    // Issue #1032: an embeddings target serves only `/v1/embeddings`. Reject
    // every other path here, before the compact/self-summarize shortcut below
    // can route it into a chat pipeline.
    if native_api == NativeApi::Embeddings && request_path != "/v1/embeddings" {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_upstream",
            "an embeddings-native endpoint target only serves POST /v1/embeddings",
        ));
    }
    if request_path == "/v1/responses/compact" {
        if compact_mode == crate::db::CompactMode::Off {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "compact_disabled",
                "POST /v1/responses/compact is disabled for this target (compact_mode=off)",
            ));
        }
        if compact_mode.is_self_summarize() && !matches!(native_api, NativeApi::Responses) {
            validate_raw_compact_request_body(request_body)?;
            return Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::BufferedBytes(request_body.to_vec()),
                response_adapter: ResponseAdapter::SelfSummarizeLocal,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            });
        }
    }
    let prepared = prepare_upstream_request_inner(
        request_path,
        request_body,
        native_api,
        provider,
        dev_system_normalize,
    )?;
    // Issue #464: per-target thinking effort override. `None`/empty means
    // inherit (follow the caller, zero-copy); an explicit value
    // force-replaces the caller value. Applied to the final (possibly
    // translated) body keyed on the final upstream path: Chat bodies gain
    // `reasoning_effort`, Responses bodies gain `reasoning.effort`, and
    // Anthropic bodies gain `output_config.effort`.
    let Some(effort) = thinking_effort_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(prepared);
    };
    let PreparedUpstreamRequest {
        path,
        body,
        response_adapter,
        upstream_redacted_request_json,
        upstream_restore_session,
    } = prepared;
    let body = match body {
        PreparedRequestBody::PassthroughStream(bytes) => PreparedRequestBody::PassthroughStream(
            apply_thinking_effort_override_for_path(&path, bytes, Some(effort)),
        ),
        PreparedRequestBody::BufferedBytes(bytes) => PreparedRequestBody::BufferedBytes(
            apply_thinking_effort_override_for_path(&path, bytes, Some(effort)),
        ),
    };
    Ok(PreparedUpstreamRequest {
        path,
        body,
        response_adapter,
        upstream_redacted_request_json,
        upstream_restore_session,
    })
}

// Issue #464: force-replace the caller thinking effort for one upstream
// body. Chat paths write `reasoning_effort`; the Responses path writes
// `reasoning.effort`; the Anthropic path writes `output_config.effort`.
// `None`/empty/invalid effort, non-JSON/non-object bodies, and an Anthropic
// `output_config` that exists but is not an object return the input unchanged.
const THINKING_EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

fn apply_thinking_effort_override_for_path(
    request_path: &str,
    body: Vec<u8>,
    effort: Option<&str>,
) -> Vec<u8> {
    let Some(effort) = effort.map(str::trim).filter(|v| !v.is_empty()) else {
        return body;
    };
    if !THINKING_EFFORTS.contains(&effort) {
        return body;
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(object) = value.as_object_mut() else {
        return body;
    };
    if request_path == "/v1/messages" {
        let effort_value = serde_json::Value::String(effort.to_string());
        match object.get_mut("output_config") {
            Some(output_config) => {
                let Some(output_config_object) = output_config.as_object_mut() else {
                    return body;
                };
                output_config_object.insert("effort".to_string(), effort_value);
            }
            None => {
                object.insert(
                    "output_config".to_string(),
                    serde_json::json!({ "effort": effort_value }),
                );
            }
        }
    } else if request_path == "/v1/responses" || request_path == "/v1/responses/compact" {
        let reasoning = object
            .entry("reasoning")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(reasoning_object) = reasoning.as_object_mut() {
            reasoning_object.insert(
                "effort".to_string(),
                serde_json::Value::String(effort.to_string()),
            );
        }
    } else if request_path == "/v1/chat/completions" {
        object.insert(
            "reasoning_effort".to_string(),
            serde_json::Value::String(effort.to_string()),
        );
    } else {
        // Issue #1032: no other path (including /v1/embeddings) receives a
        // thinking-effort field.
        return body;
    }
    serde_json::to_vec(&value).unwrap_or(body)
}

/// Issue #1032: validate an OpenAI-compatible embeddings request body before
/// it reaches the upstream.
///
/// The standard shape is enforced: a required non-empty `model` string, a
/// required `input` that is a non-empty text string, a homogeneous batch of
/// non-empty text strings, a batch of nonnegative token ids, or a batch of
/// token-id arrays; an optional `dimensions` that must be a positive i32 (no
/// string/boolean/fraction/zero/negative/overflow, never clamped); an
/// optional `encoding_format` of `float` or `base64`. `stream: true` is
/// rejected because embeddings have no streaming variant. Unknown
/// OpenAI-compatible fields are preserved by construction (the validator only
/// reads). A `null` `dimensions`/`encoding_format` is treated as absent.
pub fn validate_embeddings_request_body(request_body: &[u8]) -> Result<(), CompatError> {
    let invalid = |message: &str| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_embeddings_request",
            message,
        )
    };
    let value: serde_json::Value = serde_json::from_slice(request_body)
        .map_err(|_| invalid("embeddings request body must be a JSON object"))?;
    let Some(object) = value.as_object() else {
        return Err(invalid("embeddings request body must be a JSON object"));
    };
    if object.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "embeddings_stream_not_supported",
            "the embeddings protocol does not support streaming",
        ));
    }
    match object.get("model").and_then(serde_json::Value::as_str) {
        Some(model) if !model.trim().is_empty() => {}
        _ => {
            return Err(invalid(
                "`model` is required and must be a non-empty string",
            ));
        }
    }
    match object.get("input") {
        Some(serde_json::Value::String(text)) if !text.is_empty() => {}
        Some(serde_json::Value::Array(items)) if !items.is_empty() => {
            validate_embeddings_input_batch(items).map_err(|message| invalid(message))?;
        }
        _ => {
            return Err(invalid(
                "`input` is required and must be non-empty text or a non-empty text/token batch",
            ));
        }
    }
    if let Some(dimensions) = object.get("dimensions")
        && !dimensions.is_null()
    {
        let Some(dimensions) = dimensions.as_i64() else {
            return Err(invalid(
                "`dimensions` must be a positive integer when present",
            ));
        };
        if !(1..=i64::from(i32::MAX)).contains(&dimensions) {
            return Err(invalid(
                "`dimensions` must be a positive integer no greater than 2147483647",
            ));
        }
    }
    if let Some(encoding_format) = object.get("encoding_format")
        && !encoding_format.is_null()
    {
        match encoding_format.as_str() {
            Some("float") | Some("base64") => {}
            _ => {
                return Err(invalid(
                    "`encoding_format` must be one of float, base64 when present",
                ));
            }
        }
    }
    Ok(())
}

fn validate_embeddings_input_batch(items: &[serde_json::Value]) -> Result<(), &'static str> {
    let all_text = items
        .iter()
        .all(|item| item.as_str().is_some_and(|text| !text.is_empty()));
    if all_text {
        return Ok(());
    }
    let all_tokens = items
        .iter()
        .all(|item| item.as_i64().is_some_and(|token| token >= 0));
    if all_tokens {
        return Ok(());
    }
    let all_token_batches = items.iter().all(|item| {
        item.as_array().is_some_and(|batch| {
            !batch.is_empty()
                && batch
                    .iter()
                    .all(|token| token.as_i64().is_some_and(|token| token >= 0))
        })
    });
    if all_token_batches {
        return Ok(());
    }
    Err("`input` batches must be homogeneous non-empty strings or nonnegative token ids")
}

fn prepare_upstream_request_inner(
    request_path: &str,
    request_body: &[u8],
    native_api: NativeApi,
    provider: Option<crate::db::EndpointProvider>,
    dev_system_normalize: bool,
) -> Result<PreparedUpstreamRequest, CompatError> {
    match (request_path, native_api) {
        ("/v1/messages", NativeApi::AnthropicMessages) => {
            validate_messages_request_body(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::PassthroughStream(request_body.to_vec()),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/messages", _) => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_upstream",
            "Anthropic /v1/messages requests require an anthropic-native endpoint",
        )),
        ("/v1/responses", NativeApi::Responses) => {
            validate_raw_responses_request_body(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::PassthroughStream(request_body.to_vec()),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/responses/compact", NativeApi::Responses) => {
            validate_raw_compact_request_body(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::PassthroughStream(request_body.to_vec()),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        (
            "/v1/responses/compact",
            NativeApi::Chat | NativeApi::AnthropicMessages | NativeApi::Auto,
        ) => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "responses_cross_protocol_unsupported",
            "POST /v1/responses/compact requires a responses-native endpoint target; enable per-target self_summarize to compact without upstream support",
        )),
        ("/v1/responses", NativeApi::Chat) => {
            reject_service_tier_for_unsupported_provider(provider, request_body)?;
            let translated = responses_stateless_request_to_chat(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: NativeApi::Chat.path().to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    NativeApi::Chat.path(),
                    &translated,
                )),
                response_adapter: ResponseAdapter::ChatToResponses,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/responses", NativeApi::AnthropicMessages | NativeApi::Auto) => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "responses_cross_protocol_unsupported",
            "POST /v1/responses requires a responses-native endpoint target; \
                 stateless responses requests may target chat-native endpoints, \
                 anthropic or auto targets remain unsupported",
        )),
        ("/v1/chat/completions", NativeApi::Chat) => {
            // Issue #392 Phase K: developer->system normalization is opt-in.
            // `false` (default) leaves `developer` untouched (strict
            // upstreams must opt in); `true` rewrites to `system`.
            let normalized = if dev_system_normalize {
                normalize_chat_request_for_native(request_body)
            } else {
                request_body.to_vec()
            };
            Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(request_path, &normalized)),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/chat/completions", NativeApi::Responses) => {
            // Issue #637: the caller field is always forwarded here
            // (baseline behavior); the configuration-based override stays
            // gated in the separate live transform.
            let translated = chat_request_to_responses(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: NativeApi::Responses.path().to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    NativeApi::Responses.path(),
                    &translated,
                )),
                response_adapter: ResponseAdapter::ResponsesToChat,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/chat/completions", NativeApi::AnthropicMessages) => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_upstream",
            "legacy /v1/chat/completions cannot be routed to an anthropic-native endpoint; use /v1/responses",
        )),
        // Issue #1032: the embeddings protocol is its own non-streaming,
        // non-translated pipeline. A chat/responses/messages request must
        // never be adapted into an embedding response, and an embeddings
        // request must never reach a chat-native target.
        ("/v1/embeddings", NativeApi::Embeddings) => {
            validate_embeddings_request_body(request_body)?;
            Ok(PreparedUpstreamRequest {
                path: request_path.to_string(),
                body: PreparedRequestBody::BufferedBytes(request_body.to_vec()),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            })
        }
        ("/v1/embeddings", _) => Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_upstream",
            "POST /v1/embeddings requires an embeddings-native endpoint target",
        )),
        _ => Ok(PreparedUpstreamRequest {
            path: request_path.to_string(),
            body: PreparedRequestBody::BufferedBytes(request_body.to_vec()),
            response_adapter: ResponseAdapter::Passthrough,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
        }),
    }
}

// Issue #637: service-tier admission for the Responses→Chat
// compatibility translation. Only supported providers (the shared
// `EndpointProvider::supports_service_tier` bit also used by the live
// transform and the route probe) accept and forward the caller field; no
// provider context authorizes nothing, so route-less/legacy callers get the
// same rejection as unsupported providers. Chat→Responses needs no gate:
// the caller field is always forwarded and the configured override stays
// gated in the live transform.
fn tier_allowed_for_provider(provider: Option<crate::db::EndpointProvider>) -> bool {
    provider.is_some_and(|provider| provider.supports_service_tier())
}

/// Mirrors the compatibility validation's meaningful-value semantics for the
/// tier field so the adapter gate matches it exactly: null/blank/empty
/// values stay meaningless and never trigger the rejection.
fn has_meaningful_tier(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::String(text) => !text.trim().is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(object) => !object.is_empty(),
        serde_json::Value::Number(_) => true,
    }
}

/// Issue #637: reject a caller `service_tier` routed to an unsupported
/// provider's chat-native upstream with the same `unsupported_feature` shape
/// the validation used before the field was allowlisted, so unrelated
/// providers keep the previous explicit 400 instead of silently forwarding
/// the field. Non-JSON bodies fall through to the translation's own
/// validation.
fn reject_service_tier_for_unsupported_provider(
    provider: Option<crate::db::EndpointProvider>,
    request_body: &[u8],
) -> Result<(), CompatError> {
    if tier_allowed_for_provider(provider) {
        return Ok(());
    }
    let meaningful = serde_json::from_slice::<serde_json::Value>(request_body)
        .ok()
        .and_then(|body| {
            body.as_object()?
                .get("service_tier")
                .filter(|tier| has_meaningful_tier(tier))
                .map(|_| ())
        })
        .is_some();
    if !meaningful {
        return Ok(());
    }
    Err(CompatError::new(
        StatusCode::BAD_REQUEST,
        "unsupported_feature",
        "responses field `service_tier` is not supported for chat-native endpoints",
    ))
}

#[cfg(test)]
mod tests {
    use super::{PreparedRequestBody, ResponseAdapter, prepare_upstream_request};
    use crate::config::NativeApi;
    use serde_json::Value;

    #[test]
    fn translates_chat_requests_for_responses_native_upstreams() {
        let prepared = prepare_upstream_request(
            "/v1/chat/completions",
            br#"{
                "model":"vision-test",
                "messages":[{"role":"user","content":[
                    {"type":"text","text":"describe"},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,AA==","detail":"high"}}
                ]}],
                "stream":false
            }"#,
            NativeApi::Responses,
            false,
            None,
        )
        .unwrap();

        assert_eq!(prepared.path, "/v1/responses");
        assert_eq!(prepared.response_adapter, ResponseAdapter::ResponsesToChat);
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat to Responses compatibility must buffer the translated body");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            body["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,AA=="
        );
        assert_eq!(body["input"][0]["content"][1]["detail"], "high");
    }

    #[test]
    fn normalizes_developer_role_for_chat_native_upstreams() {
        let prepared = prepare_upstream_request(
            "/v1/chat/completions",
            br#"{
                "model":"deepseek-v4-pro",
                "messages":[
                    {"role":"developer","content":"be concise"},
                    {"role":"user","content":"hello"}
                ],
                "reasoning_effort":"max"
            }"#,
            NativeApi::Chat,
            true,
            None,
        )
        .unwrap();

        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat-native requests should be buffered");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["messages"][0]["role"].as_str(), Some("system"));
        assert_eq!(body["messages"][1]["role"].as_str(), Some("user"));
        assert_eq!(body["reasoning_effort"].as_str(), Some("max"));
    }

    #[test]
    fn default_off_passthrough_leaves_developer_untouched() {
        // Issue #392 Phase K: default-off passthrough locks the behavior
        // change — `false` must leave `developer` as-is so strict upstreams
        // opt in explicitly.
        let prepared = prepare_upstream_request(
            "/v1/chat/completions",
            br#"{
                "model":"deepseek-v4-pro",
                "messages":[
                    {"role":"developer","content":"be concise"},
                    {"role":"user","content":"hello"}
                ]
            }"#,
            NativeApi::Chat,
            false,
            None,
        )
        .unwrap();

        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat-native requests should be buffered");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["messages"][0]["role"].as_str(), Some("developer"));
        assert_eq!(body["messages"][0]["content"].as_str(), Some("be concise"));
        assert_eq!(body["messages"][1]["role"].as_str(), Some("user"));
    }

    #[test]
    fn passes_anthropic_messages_to_anthropic_native_upstreams() {
        let body = br#"{
            "model":"MiniMax-M3",
            "max_tokens":32,
            "thinking":{"type":"adaptive"},
            "messages":[
                {"role":"assistant","content":[
                    {"type":"thinking","thinking":"inspect the repository","signature":"sig-1"},
                    {"type":"tool_use","id":"toolu_1","name":"read","input":{"path":"Cargo.toml"}}
                ]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"toolu_1","content":"workspace"}
                ]}
            ]
        }"#;
        let prepared = prepare_upstream_request(
            "/v1/messages",
            body,
            NativeApi::AnthropicMessages,
            false,
            None,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/messages");
        assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
        let PreparedRequestBody::PassthroughStream(forwarded) = prepared.body else {
            panic!("Anthropic Messages requests should be forwarded unchanged");
        };
        assert_eq!(forwarded, body);
    }

    #[test]
    fn anthropic_effort_override_inserts_output_config() {
        let body = br#"{
            "model":"MiniMax-M3",
            "max_tokens":32,
            "thinking":{"type":"adaptive"},
            "messages":[{"role":"user","content":"hi"}]
        }"#;
        let prepared = prepare_upstream_request(
            "/v1/messages",
            body,
            NativeApi::AnthropicMessages,
            false,
            Some("high"),
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/messages");
        assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
        let PreparedRequestBody::PassthroughStream(bytes) = prepared.body else {
            panic!("Anthropic Messages requests should stay passthrough");
        };
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["output_config"]["effort"].as_str(), Some("high"));
        assert_eq!(body["thinking"]["type"].as_str(), Some("adaptive"));
        assert_eq!(body["max_tokens"].as_u64(), Some(32));
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn anthropic_effort_override_replaces_effort_and_keeps_siblings() {
        let body = br#"{
            "model":"MiniMax-M3",
            "max_tokens":32,
            "thinking":{"type":"adaptive","budget_tokens":1024},
            "output_config":{"effort":"low","format":{"type":"json"}},
            "messages":[{"role":"user","content":"hi"}]
        }"#;
        let prepared = super::prepare_upstream_request_for_provider(
            "/v1/messages",
            body,
            NativeApi::AnthropicMessages,
            Some(crate::db::EndpointProvider::Minimax),
            false,
            Some(" max "),
            crate::db::CompactMode::Passthrough,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/messages");
        assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
        let PreparedRequestBody::PassthroughStream(bytes) = prepared.body else {
            panic!("Anthropic Messages requests should stay passthrough");
        };
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["output_config"]["effort"].as_str(), Some("max"));
        assert_eq!(
            body["output_config"]["format"]["type"].as_str(),
            Some("json")
        );
        assert_eq!(body["thinking"]["type"].as_str(), Some("adaptive"));
        assert_eq!(body["thinking"]["budget_tokens"].as_u64(), Some(1024));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_effort_override_preserves_non_object_output_config() {
        for body in [
            br#"{"model":"m","max_tokens":32,"messages":[{"role":"user","content":"hi"}],"output_config":"compact"}"#.as_slice(),
            br#"{"model":"m","max_tokens":32,"messages":[{"role":"user","content":"hi"}],"output_config":["effort"]}"#.as_slice(),
            br#"{"model":"m","max_tokens":32,"messages":[{"role":"user","content":"hi"}],"output_config":null}"#.as_slice(),
        ] {
            let prepared = prepare_upstream_request(
                "/v1/messages",
                body,
                NativeApi::AnthropicMessages,
                false,
                Some("high"),
            )
            .unwrap();
            let PreparedRequestBody::PassthroughStream(bytes) = prepared.body else {
                panic!("Anthropic Messages requests should stay passthrough");
            };
            assert_eq!(bytes, body);
        }
    }

    #[test]
    fn anthropic_effort_override_ignores_inherit_blank_and_invalid() {
        let body = br#"{"model":"m","max_tokens":32,"thinking":{"type":"adaptive"},"messages":[{"role":"user","content":"hi"}]}"#;
        for effort in [None, Some(""), Some("   "), Some("ultra")] {
            let prepared = prepare_upstream_request(
                "/v1/messages",
                body,
                NativeApi::AnthropicMessages,
                false,
                effort,
            )
            .unwrap();
            let PreparedRequestBody::PassthroughStream(bytes) = prepared.body else {
                panic!("Anthropic Messages requests should stay passthrough");
            };
            assert_eq!(
                bytes, body,
                "effort {effort:?} must preserve the input bytes"
            );
        }
    }

    #[test]
    fn anthropic_effort_override_preserves_malformed_and_non_object_bodies() {
        // The public Anthropic path validates JSON before the override, so the
        // helper guards are exercised directly for these byte-identity cases.
        for body in [
            br#"{"model":"m","#.as_slice(),
            br#"[{"model":"m"}]"#.as_slice(),
            br#"null"#.as_slice(),
        ] {
            let result = super::apply_thinking_effort_override_for_path(
                "/v1/messages",
                body.to_vec(),
                Some("high"),
            );
            assert_eq!(result, body);
        }
    }

    #[test]
    fn rejects_anthropic_messages_for_openai_native_upstreams() {
        let error = prepare_upstream_request(
            "/v1/messages",
            br#"{"model":"claude-sonnet","max_tokens":32,"messages":[{"role":"user","content":"hi"}]}"#,
            NativeApi::Responses,
            false,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, "unsupported_upstream");
    }

    #[test]
    fn thinking_effort_override_force_replaces_chat_value() {
        // Issue #464: an explicit override force-replaces the caller value;
        // inherit (`None`) passes the caller value through untouched.
        for (caller, effort, expected) in [
            (
                r#"{"model":"m","reasoning_effort":"low"}"#,
                Some("high"),
                "high",
            ),
            (r#"{"model":"m"}"#, Some("high"), "high"),
            (r#"{"model":"m","reasoning_effort":"low"}"#, None, "low"),
        ] {
            let prepared = prepare_upstream_request(
                "/v1/chat/completions",
                caller.as_bytes(),
                NativeApi::Chat,
                false,
                effort,
            )
            .unwrap();
            let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
                panic!("chat-native requests should be buffered");
            };
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["reasoning_effort"].as_str(), Some(expected));
        }
        // Inherit with no caller value leaves the key absent.
        let prepared = prepare_upstream_request(
            "/v1/chat/completions",
            br#"{"model":"m"}"#,
            NativeApi::Chat,
            false,
            None,
        )
        .unwrap();
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat-native requests should be buffered");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn thinking_effort_override_writes_responses_effort() {
        // Issue #464: Responses-native bodies gain `reasoning.effort`;
        // Chat->Responses translation lands on `reasoning.effort`;
        // Responses->Chat translation lands on `reasoning_effort`.
        let prepared = prepare_upstream_request(
            "/v1/responses",
            br#"{"model":"m","reasoning":{"effort":"low"}}"#,
            NativeApi::Responses,
            false,
            Some("high"),
        )
        .unwrap();
        let PreparedRequestBody::PassthroughStream(body) = prepared.body else {
            panic!("responses-native requests should stream");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["reasoning"]["effort"].as_str(), Some("high"));

        let prepared = prepare_upstream_request(
            "/v1/chat/completions",
            br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"reasoning_effort":"low"}"#,
            NativeApi::Responses,
            false,
            Some("high"),
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/responses");
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat to Responses translation must buffer");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["reasoning"]["effort"].as_str(), Some("high"));

        let prepared = prepare_upstream_request(
            "/v1/responses",
            br#"{"model":"m","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],"reasoning":{"effort":"low"}}"#,
            NativeApi::Chat,
            false,
            Some("high"),
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/chat/completions");
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("responses to chat translation must buffer");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["reasoning_effort"].as_str(), Some("high"));
    }

    #[test]
    fn compact_passes_through_for_responses_native_upstreams() {
        let body = br#"{"model":"m","input":[{"type":"message","role":"user","content":"hi"}]}"#;
        let prepared = prepare_upstream_request(
            "/v1/responses/compact",
            body,
            NativeApi::Responses,
            false,
            None,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/responses/compact");
        assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
        let PreparedRequestBody::PassthroughStream(forwarded) = prepared.body else {
            panic!("compact requests should stream unchanged");
        };
        assert_eq!(forwarded, body);
    }

    #[test]
    fn compact_rejects_chat_native_upstreams() {
        let error = prepare_upstream_request(
            "/v1/responses/compact",
            br#"{"model":"m","input":[{"type":"message","role":"user","content":"hi"}]}"#,
            NativeApi::Chat,
            false,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, "responses_cross_protocol_unsupported");
    }

    #[test]
    fn compact_off_rejects_all_targets() {
        use super::prepare_upstream_request_with_compact;
        use crate::db::CompactMode;
        for native_api in [
            NativeApi::Responses,
            NativeApi::Chat,
            NativeApi::AnthropicMessages,
        ] {
            let error = prepare_upstream_request_with_compact(
                "/v1/responses/compact",
                br#"{"model":"m","input":"hi"}"#,
                native_api,
                false,
                None,
                CompactMode::Off,
            )
            .unwrap_err();
            assert_eq!(error.code, "compact_disabled");
        }
    }

    #[test]
    fn compact_self_summarize_routes_chat_to_local_flow() {
        use super::prepare_upstream_request_with_compact;
        use crate::db::CompactMode;
        let body = br#"{"model":"m","input":[{"type":"message","role":"user","content":"hi"}]}"#;
        let prepared = prepare_upstream_request_with_compact(
            "/v1/responses/compact",
            body,
            NativeApi::Chat,
            false,
            None,
            CompactMode::SelfSummarize,
        )
        .unwrap();
        assert_eq!(
            prepared.response_adapter,
            ResponseAdapter::SelfSummarizeLocal
        );
        let PreparedRequestBody::BufferedBytes(forwarded) = prepared.body else {
            panic!("self-summarize compact should buffer the request body");
        };
        assert_eq!(forwarded, body);
    }

    #[test]
    fn compact_self_summarize_keeps_responses_passthrough() {
        use super::prepare_upstream_request_with_compact;
        use crate::db::CompactMode;
        let body = br#"{"model":"m","input":"hi"}"#;
        let prepared = prepare_upstream_request_with_compact(
            "/v1/responses/compact",
            body,
            NativeApi::Responses,
            false,
            None,
            CompactMode::SelfSummarize,
        )
        .unwrap();
        assert_eq!(prepared.response_adapter, ResponseAdapter::Passthrough);
    }

    #[test]
    fn responses_to_chat_rejects_tier_for_unsupported_provider() {
        // Issue #637: unrelated providers keep the previous explicit 400
        // instead of silently forwarding the caller field.
        let error = super::prepare_upstream_request_for_provider(
            "/v1/responses",
            br#"{"model":"m","input":"hi","service_tier":"priority"}"#,
            NativeApi::Chat,
            Some(crate::db::EndpointProvider::Generic),
            false,
            None,
            crate::db::CompactMode::Passthrough,
        )
        .unwrap_err();
        assert_eq!(error.code, "unsupported_feature");
        assert!(error.message.contains("service_tier"));
    }

    #[test]
    fn responses_to_chat_forwards_tier_for_supported_providers() {
        // Issue #637: MiniMax/OpenAI chat-native targets accept the caller
        // field.
        for provider in [
            crate::db::EndpointProvider::Minimax,
            crate::db::EndpointProvider::OpenAi,
        ] {
            let prepared = super::prepare_upstream_request_for_provider(
                "/v1/responses",
                br#"{"model":"m","input":"hi","service_tier":"priority"}"#,
                NativeApi::Chat,
                Some(provider),
                false,
                None,
                crate::db::CompactMode::Passthrough,
            )
            .unwrap();
            assert_eq!(prepared.path, "/v1/chat/completions");
            let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
                panic!("responses to chat translation must buffer");
            };
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["service_tier"].as_str(), Some("priority"));
        }
    }

    #[test]
    fn chat_to_responses_forwards_caller_tier_for_unsupported_provider() {
        // Issue #637: caller-compatibility baseline — the caller field is
        // always forwarded on Chat→Responses (plan decision 3: no
        // configured value preserves the caller's field). The
        // configuration-based override stays gated in the separate live
        // transform, which never injects for unsupported combinations.
        let prepared = super::prepare_upstream_request_for_provider(
            "/v1/chat/completions",
            br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"service_tier":"priority"}"#,
            NativeApi::Responses,
            Some(crate::db::EndpointProvider::Generic),
            false,
            None,
            crate::db::CompactMode::Passthrough,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/responses");
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat to Responses translation must buffer");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["service_tier"].as_str(), Some("priority"));
    }

    #[test]
    fn chat_to_responses_keeps_tier_for_supported_provider() {
        let prepared = super::prepare_upstream_request_for_provider(
            "/v1/chat/completions",
            br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"service_tier":"fast"}"#,
            NativeApi::Responses,
            Some(crate::db::EndpointProvider::OpenAi),
            false,
            None,
            crate::db::CompactMode::Passthrough,
        )
        .unwrap();
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat to Responses translation must buffer");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["service_tier"].as_str(), Some("fast"));
    }

    #[test]
    fn routeless_requests_reject_tier_on_responses_to_chat_only() {
        // Issue #637: no provider context authorizes nothing on
        // Responses→Chat (same rejection as unsupported providers), while
        // Chat→Responses forwards the caller field for every provider
        // (caller-compatibility baseline; overrides stay gated in the live
        // transform).
        let error = super::prepare_upstream_request_for_provider(
            "/v1/responses",
            br#"{"model":"m","input":"hi","service_tier":"priority"}"#,
            NativeApi::Chat,
            None,
            false,
            None,
            crate::db::CompactMode::Passthrough,
        )
        .unwrap_err();
        assert_eq!(error.code, "unsupported_feature");
        assert!(error.message.contains("service_tier"));

        let prepared = super::prepare_upstream_request_for_provider(
            "/v1/chat/completions",
            br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"service_tier":"priority"}"#,
            NativeApi::Responses,
            None,
            false,
            None,
            crate::db::CompactMode::Passthrough,
        )
        .unwrap();
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("chat to Responses translation must buffer");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["service_tier"].as_str(), Some("priority"));
    }

    #[test]
    fn embeddings_requests_are_validated_and_forwarded_untranslated() {
        // Issue #1032: the embeddings arm buffers the body unchanged and never
        // applies a chat translation.
        let prepared = super::prepare_upstream_request(
            "/v1/embeddings",
            br#"{"model":"qwen/qwen3-embedding-8b","input":["a","b"],"dimensions":7,"unknown_field":true}"#,
            NativeApi::Embeddings,
            false,
            None,
        )
        .unwrap();
        assert_eq!(prepared.path, "/v1/embeddings");
        assert!(matches!(
            prepared.response_adapter,
            ResponseAdapter::Passthrough
        ));
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("embeddings must be buffered");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["dimensions"], 7);
        assert_eq!(body["unknown_field"], true);
    }

    #[test]
    fn embeddings_request_validation_rejects_malformed_shapes() {
        let prepare = |body: &str| {
            super::prepare_upstream_request(
                "/v1/embeddings",
                body.as_bytes(),
                NativeApi::Embeddings,
                false,
                None,
            )
        };
        // Valid shapes are accepted.
        assert!(prepare(r#"{"model":"m","input":"hello"}"#).is_ok());
        assert!(prepare(r#"{"model":"m","input":[1,2,3]}"#).is_ok());
        assert!(prepare(r#"{"model":"m","input":[[1,2],[3]]}"#).is_ok());
        assert!(prepare(r#"{"model":"m","input":"x","encoding_format":"base64"}"#).is_ok());
        assert!(prepare(r#"{"model":"m","input":"x","dimensions":null}"#).is_ok());
        // Rejected shapes.
        for body in [
            r#"{"input":"hello"}"#,
            r#"{"model":"","input":"hello"}"#,
            r#"{"model":"m"}"#,
            r#"{"model":"m","input":""}"#,
            r#"{"model":"m","input":[]}"#,
            r#"{"model":"m","input":["a",1]}"#,
            r#"{"model":"m","input":[-1]}"#,
            r#"{"model":"m","input":"x","dimensions":0}"#,
            r#"{"model":"m","input":"x","dimensions":-1}"#,
            r#"{"model":"m","input":"x","dimensions":1.5}"#,
            r#"{"model":"m","input":"x","dimensions":2147483648}"#,
            r#"{"model":"m","input":"x","dimensions":"7"}"#,
            r#"{"model":"m","input":"x","encoding_format":"hex"}"#,
        ] {
            let error = prepare(body).expect_err("must be rejected");
            assert_eq!(error.code, "invalid_embeddings_request", "body {body}");
        }
        let error = prepare(r#"{"model":"m","input":"x","stream":true}"#)
            .expect_err("streaming must be rejected");
        assert_eq!(error.code, "embeddings_stream_not_supported");
    }

    #[test]
    fn an_embeddings_target_only_serves_the_embeddings_path() {
        // Issue #1032: a chat request is never adapted into an embedding
        // response and vice versa.
        let error = super::prepare_upstream_request(
            "/v1/chat/completions",
            br#"{"model":"m","messages":[]}"#,
            NativeApi::Embeddings,
            false,
            None,
        )
        .expect_err("a chat request to an embeddings target must be rejected");
        assert_eq!(error.code, "unsupported_upstream");

        let error = super::prepare_upstream_request(
            "/v1/embeddings",
            br#"{"model":"m","input":"x"}"#,
            NativeApi::Chat,
            false,
            None,
        )
        .expect_err("an embeddings request to a chat target must be rejected");
        assert_eq!(error.code, "unsupported_upstream");
    }

    #[test]
    fn an_embeddings_body_never_gains_a_thinking_effort_field() {
        // Issue #1032: a stored chat override is never applied to embeddings.
        let prepared = super::prepare_upstream_request(
            "/v1/embeddings",
            br#"{"model":"m","input":"x"}"#,
            NativeApi::Embeddings,
            false,
            Some("high"),
        )
        .unwrap();
        let PreparedRequestBody::BufferedBytes(body) = prepared.body else {
            panic!("embeddings must be buffered");
        };
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("reasoning_effort").is_none());
    }
}
