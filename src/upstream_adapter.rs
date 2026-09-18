use http::StatusCode;

use crate::{
    anthropic_compat::validate_messages_request_body,
    config::NativeApi,
    openai_compat::{
        CompatError, chat_request_to_responses, normalize_chat_request_for_native,
        responses_stateless_request_to_chat, validate_raw_responses_request_body,
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
    let prepared = prepare_upstream_request_inner(
        request_path,
        request_body,
        native_api,
        dev_system_normalize,
    )?;
    // Issue #464: per-target thinking effort override. `None`/empty means
    // inherit (follow the caller, zero-copy); an explicit value
    // force-replaces the caller value. Applied to the final (possibly
    // translated) body keyed on the final upstream path: Chat bodies gain
    // `reasoning_effort`, Responses bodies gain `reasoning.effort`.
    // Anthropic `/v1/messages` is untouched.
    if prepared.path == "/v1/messages" {
        return Ok(prepared);
    }
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
// `reasoning.effort`. `None`/empty/invalid effort or non-JSON/non-object
// bodies return the input unchanged.
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
    if request_path == "/v1/responses" {
        let reasoning = object
            .entry("reasoning")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(reasoning_object) = reasoning.as_object_mut() {
            reasoning_object.insert(
                "effort".to_string(),
                serde_json::Value::String(effort.to_string()),
            );
        }
    } else {
        object.insert(
            "reasoning_effort".to_string(),
            serde_json::Value::String(effort.to_string()),
        );
    }
    serde_json::to_vec(&value).unwrap_or(body)
}

fn prepare_upstream_request_inner(
    request_path: &str,
    request_body: &[u8],
    native_api: NativeApi,
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
        ("/v1/responses", NativeApi::Chat) => {
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
        _ => Ok(PreparedUpstreamRequest {
            path: request_path.to_string(),
            body: PreparedRequestBody::BufferedBytes(request_body.to_vec()),
            response_adapter: ResponseAdapter::Passthrough,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
        }),
    }
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
}
