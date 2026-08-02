use anyhow::{Error, Result};
use serde_json::Value;

use crate::{
    openai_compat::CompatError,
    redact_upstream::{
        UpstreamRedactedRequest, UpstreamRedactionProcessor, UpstreamRedactionSession,
    },
};

use super::upstream_text_fields::should_process_ai_string_field;
use crate::worker::runtime::json_walker::walk_json_strings;

pub(super) type PreparedRedactedRequest = UpstreamRedactedRequest;

pub(super) fn redact_ai_request_json(
    path: &str,
    body: &[u8],
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<&UpstreamRedactionSession>,
) -> Result<PreparedRedactedRequest, CompatError> {
    let mut value: Value = serde_json::from_slice(body).map_err(|err| {
        CompatError::new(
            reqwest::StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("invalid request json for upstream redaction: {err}"),
        )
    })?;
    let external_id = conversation_id.as_ref().map(uuid::Uuid::to_string);
    let mut processor =
        UpstreamRedactionProcessor::new(user_id, external_id.as_deref(), prior_session).map_err(
            |err| {
                CompatError::new(
                    reqwest::StatusCode::BAD_REQUEST,
                    "redaction_failed",
                    format!("failed to initialize upstream redaction: {err}"),
                )
            },
        )?;
    walk_json_strings(&mut value, |context, text| {
        let field_name = context.field_name.unwrap_or_default();
        if !should_process_ai_string_field(path, context.json_path, context.object_type, field_name)
        {
            return Ok(None);
        }
        processor
            .redact_fragment(text, redactor::InputKind::Text)
            .map(Some)
            .map_err(Error::new)
    })
    .map_err(|err| {
        CompatError::new(
            reqwest::StatusCode::BAD_REQUEST,
            "redaction_failed",
            format!("failed to redact upstream request: {err}"),
        )
    })?;
    let redacted_body = serde_json::to_vec(&value).map_err(|err| {
        CompatError::new(
            reqwest::StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("failed to encode redacted upstream request: {err}"),
        )
    })?;
    let original_text = std::str::from_utf8(body).map_err(|err| {
        CompatError::new(
            reqwest::StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("invalid request text for upstream redaction: {err}"),
        )
    })?;
    let redacted_text = std::str::from_utf8(&redacted_body).expect("serialized JSON is UTF-8");
    let applied_replacements = processor.has_applied_replacements();
    let request_session = processor
        .finish_state(original_text, redacted_text)
        .map_err(|err| {
            CompatError::new(
                reqwest::StatusCode::BAD_REQUEST,
                "redaction_failed",
                format!("failed to finalize upstream redaction: {err}"),
            )
        })?;
    let redacted_request_json = applied_replacements.then_some(value);
    Ok(PreparedRedactedRequest {
        body: redacted_body,
        redacted_request_json,
        restore_session: request_session,
    })
}

pub(super) async fn redact_ai_request_json_blocking(
    path: String,
    body: Vec<u8>,
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<UpstreamRedactionSession>,
) -> Result<PreparedRedactedRequest, CompatError> {
    tokio::task::spawn_blocking(move || {
        redact_ai_request_json(
            &path,
            &body,
            user_id,
            conversation_id,
            prior_session.as_ref(),
        )
    })
    .await
    .map_err(|err| {
        CompatError::new(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "redaction_task_failed",
            format!("upstream redaction task failed: {err}"),
        )
    })?
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::{redact_ai_request_json, redact_ai_request_json_blocking};
    use crate::redact_test_support::domain_redaction;
    use serde_json::{Value, json};

    #[test]
    fn redacts_responses_text_fields_without_touching_control_fields() {
        let _guard = domain_redaction();

        let body = json!({
            "model": "gpt-test",
            "instructions": "contact a.example.com",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "lookup",
                    "arguments": "{\"email\":\"a.example.com\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "result from b.example.com"
                },
                {
                    "role": "user",
                    "content": "ask c.example.com"
                }
            ]
        });

        let prepared = redact_ai_request_json(
            "/v1/responses",
            body.to_string().as_bytes(),
            None,
            None,
            None,
        )
        .expect("redact");
        let redacted: Value = serde_json::from_slice(&prepared.body).expect("json");

        assert_eq!(redacted["model"].as_str(), Some("gpt-test"));
        assert_eq!(redacted["input"][0]["call_id"].as_str(), Some("call_1"));
        assert_eq!(redacted["input"][0]["name"].as_str(), Some("lookup"));
        assert!(
            redacted["instructions"]
                .as_str()
                .expect("instructions")
                .contains("[[RDX:v2:")
        );
        assert!(
            redacted["input"][0]["arguments"]
                .as_str()
                .expect("arguments")
                .contains("[[RDX:v2:")
        );
        assert!(
            redacted["input"][1]["output"]
                .as_str()
                .expect("output")
                .contains("[[RDX:v2:")
        );
        let session = prepared.restore_session.expect("restore session");
        assert_eq!(session.request_session().entries.len(), 3);
        assert_eq!(
            session.request_session().redacted_text,
            String::from_utf8(prepared.body).expect("UTF-8 JSON")
        );
    }

    #[test]
    fn redacts_nested_chat_tool_call_arguments() {
        let _guard = domain_redaction();

        let body = json!({
            "messages": [
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "lookup",
                                "arguments": "{\"domain\":\"a.example.com\"}"
                            }
                        }
                    ]
                }
            ]
        });

        let prepared = redact_ai_request_json(
            "/v1/chat/completions",
            body.to_string().as_bytes(),
            None,
            None,
            None,
        )
        .expect("redact");
        let redacted: Value = serde_json::from_slice(&prepared.body).expect("json");

        assert_eq!(
            redacted["messages"][0]["tool_calls"][0]["function"]["name"].as_str(),
            Some("lookup"),
        );
        assert!(
            redacted["messages"][0]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .expect("arguments")
                .contains("[[RDX:v2:")
        );
    }

    #[test]
    fn does_not_keep_full_json_when_prior_session_has_no_new_replacements() {
        let _guard = domain_redaction();

        let first = redact_ai_request_json(
            "/v1/responses",
            br#"{"instructions":"contact a.example.com"}"#,
            None,
            None,
            None,
        )
        .expect("first redaction");
        assert!(first.redacted_request_json.is_some());
        let prior_session = first.restore_session.expect("first session");

        let second = redact_ai_request_json(
            "/v1/responses",
            br#"{"instructions":"continue"}"#,
            None,
            None,
            Some(&prior_session),
        )
        .expect("second redaction");
        assert!(second.restore_session.is_some());
        assert!(second.redacted_request_json.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_redaction_yields_to_the_async_runtime() {
        {
            let _guard = domain_redaction();
        }
        let body = serde_json::to_vec(&json!({
            "model": "gpt-test",
            "instructions": "contact a.example.com ".repeat(1_000),
        }))
        .expect("request JSON");
        let completed = Arc::new(AtomicBool::new(false));
        let completed_after_redaction = Arc::clone(&completed);

        let redaction = async move {
            let result = redact_ai_request_json_blocking(
                "/v1/responses".to_string(),
                body,
                None,
                None,
                None,
            )
            .await;
            completed_after_redaction.store(true, Ordering::SeqCst);
            result
        };
        let observer = async {
            assert!(!completed.load(Ordering::SeqCst));
        };
        let (result, ()) = tokio::join!(redaction, observer);

        result.expect("redaction");
    }
}
