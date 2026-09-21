use crate::upstream_error::MappedInvalid;
use serde_json::Value;

/// Rewrite the client-facing error envelope for a recoverable upstream
/// continuation failure: keep the original message, append the retry hint, and
/// stamp the retryable code. No-op when the envelope carries no `error`
/// object, so unknown envelope shapes keep their upstream payload.
pub(super) fn apply_hint(envelope: &mut Value, mapped: &MappedInvalid) {
    let Some(object) = envelope.get_mut("error").and_then(Value::as_object_mut) else {
        return;
    };
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let message = if message.is_empty() {
        format!("retry: {}", mapped.hint)
    } else {
        format!("{message} | retry: {}", mapped.hint)
    };
    object.insert("message".to_string(), Value::String(message));
    object.insert("code".to_string(), Value::String(mapped.code.to_string()));
}

#[cfg(test)]
mod tests {
    use super::apply_hint;
    use crate::upstream_error::map_upstream_invalid_request;
    use serde_json::json;

    #[test]
    fn appends_hint_to_the_openai_error_envelope() {
        let mut envelope = json!({
            "error": {
                "message": "No tool output found for function call call_abc",
                "type": "invalid_request_error",
                "code": null,
            }
        });

        let mapped =
            map_upstream_invalid_request("No tool output found for function call call_abc")
                .expect("retryable invalid request");
        apply_hint(&mut envelope, &mapped);

        let error = &envelope["error"];
        assert_eq!(error["code"], "retryable_invalid_continuation");
        assert_eq!(error["type"], "invalid_request_error");
        let message = error["message"].as_str().expect("message");
        assert!(message.contains("No tool output found for function call call_abc"));
        assert!(message.contains(" | retry: "));
        assert!(message.contains("previous_response_id"));
    }

    #[test]
    fn appends_hint_to_the_anthropic_error_envelope() {
        let mut envelope = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Referenced reasoning item 'rs_abc:rs_xyz' was not found or has expired",
            }
        });

        let mapped = map_upstream_invalid_request(
            "Referenced reasoning item 'rs_abc:rs_xyz' was not found or has expired",
        )
        .expect("retryable invalid request");
        apply_hint(&mut envelope, &mapped);

        assert_eq!(envelope["type"], "error");
        assert_eq!(envelope["error"]["code"], "retryable_invalid_continuation");
        assert_eq!(envelope["error"]["type"], "invalid_request_error");
        assert!(
            envelope["error"]["message"]
                .as_str()
                .expect("message")
                .contains("previous_response_id")
        );
    }

    #[test]
    fn fills_a_missing_message_and_leaves_absent_error_objects_untouched() {
        let mut envelope = json!({ "error": { "type": "invalid_request_error" } });
        let mapped =
            map_upstream_invalid_request("No tool output found for function call call_x").unwrap();
        apply_hint(&mut envelope, &mapped);
        assert!(
            envelope["error"]["message"]
                .as_str()
                .expect("message")
                .starts_with("retry: ")
        );

        let mut untouched = json!({ "message": "No tool output found for function call call_x" });
        apply_hint(&mut untouched, &mapped);
        assert_eq!(
            untouched,
            json!({ "message": "No tool output found for function call call_x" })
        );
    }
}
