use serde_json::{Value, json};

pub(crate) fn normalize_response_error(body_text: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(body_text) {
        if value.get("error").is_some() {
            return value;
        }

        let code = value.get("code").cloned().unwrap_or(Value::Null);
        let error_type = value
            .get("type")
            .cloned()
            .unwrap_or_else(|| Value::String("invalid_request_error".to_string()));
        let param = value.get("param").cloned().unwrap_or(Value::Null);

        if let Some(detail) = value.get("detail").and_then(Value::as_str) {
            return json!({
                "error": {
                    "message": detail,
                    "type": error_type,
                    "param": param,
                    "code": code,
                }
            });
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return json!({
                "error": {
                    "message": message,
                    "type": error_type,
                    "param": param,
                    "code": code,
                }
            });
        }
    }
    json!({
        "error": {
            "message": body_text.trim(),
            "type": "invalid_request_error",
            "param": Value::Null,
            "code": Value::Null,
        }
    })
}
