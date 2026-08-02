use anyhow::{Error, Result};
use serde_json::Value;

use crate::{
    redact_upstream::{
        UpstreamRedactedRequest, UpstreamRedactionProcessor, UpstreamRedactionSession,
    },
    worker::runtime::json_walker::walk_json_strings,
};

pub(super) fn redact_mcp_request_body(
    body: &[u8],
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<&UpstreamRedactionSession>,
) -> Result<UpstreamRedactedRequest> {
    let mut value: Value = serde_json::from_slice(body)?;
    let external_id = conversation_id.as_ref().map(uuid::Uuid::to_string);
    let mut processor =
        UpstreamRedactionProcessor::new(user_id, external_id.as_deref(), prior_session)?;
    walk_json_strings(&mut value, |context, text| {
        if !should_redact_mcp_string(context.json_path) {
            return Ok(None);
        }
        processor
            .redact_fragment(text, redactor::InputKind::Text)
            .map(Some)
            .map_err(Error::new)
    })?;
    let redacted_body = serde_json::to_vec(&value)?;
    let original_text = std::str::from_utf8(body)?;
    let redacted_text = std::str::from_utf8(&redacted_body).expect("serialized JSON is UTF-8");
    let request_session = processor.finish_state(original_text, redacted_text)?;
    Ok(UpstreamRedactedRequest {
        body: redacted_body,
        redacted_request_json: processor.has_applied_replacements().then_some(value),
        restore_session: request_session,
    })
}

pub(super) async fn redact_mcp_request_body_blocking(
    body: Vec<u8>,
    user_id: Option<i64>,
    conversation_id: Option<uuid::Uuid>,
    prior_session: Option<UpstreamRedactionSession>,
) -> Result<UpstreamRedactedRequest> {
    tokio::task::spawn_blocking(move || {
        redact_mcp_request_body(&body, user_id, conversation_id, prior_session.as_ref())
    })
    .await?
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
    use crate::{redact_test_support::domain_redaction, redact_upstream::restore_text};
    use serde_json::{Value, json};

    #[test]
    fn redacts_body_text_but_preserves_protocol_fields() {
        let _guard = domain_redaction();

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

        let prepared =
            redact_mcp_request_body(body.to_string().as_bytes(), None, None, None).expect("redact");
        let bytes = prepared.body;
        let session = prepared.restore_session;
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
