use anyhow::Result;
use serde_json::Value;

use crate::redact_upstream::{UpstreamRedactionProcessor, UpstreamRedactionSession};

pub(super) fn redact_mcp_request_body(
    body: &[u8],
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<&UpstreamRedactionSession>,
) -> Result<(Vec<u8>, Option<Value>, Option<UpstreamRedactionSession>)> {
    let mut value: Value = serde_json::from_slice(body)?;
    let external_id = conversation_id.as_ref().map(uuid::Uuid::to_string);
    let mut processor =
        UpstreamRedactionProcessor::new(user_id, external_id.as_deref(), prior_session)?;
    redact_mcp_value(&mut value, "", &mut processor);
    let redacted_body = serde_json::to_vec(&value)?;
    let original_text = std::str::from_utf8(body)?;
    let redacted_text = std::str::from_utf8(&redacted_body).expect("serialized JSON is UTF-8");
    let request_session = processor.finish_state(original_text, redacted_text)?;
    Ok((redacted_body, Some(value), request_session))
}

pub(super) async fn redact_mcp_request_body_blocking(
    body: Vec<u8>,
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<UpstreamRedactionSession>,
) -> Result<(Vec<u8>, Option<Value>, Option<UpstreamRedactionSession>)> {
    tokio::task::spawn_blocking(move || {
        redact_mcp_request_body(&body, user_id, conversation_id, prior_session.as_ref())
    })
    .await?
}

fn redact_mcp_value(
    value: &mut Value,
    json_path: &str,
    processor: &mut UpstreamRedactionProcessor,
) {
    match value {
        Value::String(text) => {
            if !should_redact_mcp_string(json_path) {
                return;
            }
            if let Ok(redacted_text) = processor.redact_fragment(text, redactor::InputKind::Text) {
                *text = redacted_text;
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                redact_mcp_value(item, &format!("{json_path}/{index}"), processor);
            }
        }
        Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                redact_mcp_value(value, &format!("{json_path}/{key}"), processor);
            }
        }
        _ => {}
    }
}

fn should_redact_mcp_string(json_path: &str) -> bool {
    !matches!(
        json_path,
        "/jsonrpc"
            | "/method"
            | "/id"
            | "/params/name"
            | "/params/uri"
            | "/params/server"
            | "/params/tool"
            | "/params/resource"
            | "/params/method"
    ) && !json_path.ends_with("/mimeType")
        && !json_path.ends_with("/type")
}

#[cfg(test)]
mod tests {
    use super::redact_mcp_request_body;
    use crate::{
        redact::{RedactionConfig, apply_config},
        redact_upstream::restore_text,
    };
    use redactor::RedactionRules;
    use serde_json::{Value, json};

    #[test]
    fn redacts_body_text_but_preserves_protocol_fields() {
        let _guard = crate::redact::TEST_REDACTION_LOCK.lock().expect("lock");
        apply_config(&RedactionConfig {
            enabled: true,
            rules: RedactionRules {
                domain: true,
                ..RedactionRules::default()
            },
            custom_strings: Vec::new(),
        })
        .expect("config");

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "lookup.alice@example.com",
                "arguments": {
                    "prompt": "email alice@example.com",
                    "note": "call bob@example.com"
                }
            }
        });

        let (bytes, _, session) =
            redact_mcp_request_body(body.to_string().as_bytes(), None, None, None).expect("redact");
        let redacted: Value = serde_json::from_slice(&bytes).expect("json");

        assert_eq!(redacted["method"].as_str(), Some("tools/call"));
        assert_eq!(
            redacted["params"]["name"].as_str(),
            Some("lookup.alice@example.com")
        );
        assert!(
            redacted["params"]["arguments"]["prompt"]
                .as_str()
                .expect("prompt")
                .contains("[[RDX:v2:")
        );
        assert!(
            redacted["params"]["arguments"]["note"]
                .as_str()
                .expect("note")
                .contains("[[RDX:v2:")
        );

        let session = session.expect("session");
        assert_eq!(session.request_session().entries.len(), 2);
        assert_eq!(
            session.request_session().redacted_text,
            String::from_utf8(bytes).expect("UTF-8 JSON")
        );
        let restored = restore_text(
            redacted["params"]["arguments"]["prompt"]
                .as_str()
                .expect("prompt"),
            &session,
        )
        .expect("restore");
        assert!(restored.is_valid());
        assert_eq!(restored.restored_text, "email alice@example.com");
    }
}
