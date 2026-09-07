use http::StatusCode;

use crate::{
    anthropic_compat::validate_messages_request_body,
    config::NativeApi,
    openai_compat::{
        CompatError, chat_request_to_responses, normalize_chat_request_for_native,
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
        ("/v1/responses", NativeApi::Chat | NativeApi::AnthropicMessages | NativeApi::Auto) => {
            Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "responses_cross_protocol_unsupported",
                "POST /v1/responses requires a responses-native endpoint target; \
                 responses requests are passed through unchanged and are never converted \
                 to chat or anthropic protocols",
            ))
        }
        ("/v1/chat/completions", NativeApi::Chat) => Ok(PreparedUpstreamRequest {
            path: request_path.to_string(),
            body: PreparedRequestBody::BufferedBytes(upstream_body(
                request_path,
                &normalize_chat_request_for_native(request_body),
            )),
            response_adapter: ResponseAdapter::Passthrough,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
        }),
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
        let prepared =
            prepare_upstream_request("/v1/messages", body, NativeApi::AnthropicMessages).unwrap();
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
        )
        .unwrap_err();
        assert_eq!(error.code, "unsupported_upstream");
    }
}
