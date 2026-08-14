use serde_json::Value;

/// Normalize common OpenAI message roles for strict OpenAI-compatible Chat upstreams.
///
/// OpenCode's Chat adapter and the DeepSeek Harness adapter both emit `system`
/// rather than `developer`. Some Chat gateways reject the latter even though
/// newer OpenAI clients may send it.
pub(crate) fn normalize_chat_request_for_native(body: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return body.to_vec();
    };

    let mut changed = false;
    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        if object.get("role").and_then(Value::as_str) != Some("developer") {
            continue;
        }
        object.insert("role".to_string(), Value::String("system".to_string()));
        changed = true;
    }
    if !changed {
        return body.to_vec();
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::normalize_chat_request_for_native;

    #[test]
    fn maps_developer_messages_to_system_messages() {
        let body = normalize_chat_request_for_native(
            br#"{"messages":[{"role":"developer","content":"be concise"},{"role":"user","content":"hi"}]}"#,
        );
        let value = serde_json::from_slice::<Value>(&body).unwrap();

        assert_eq!(value["messages"][0]["role"].as_str(), Some("system"));
        assert_eq!(value["messages"][0]["content"].as_str(), Some("be concise"));
        assert_eq!(value["messages"][1]["role"].as_str(), Some("user"));
    }

    #[test]
    fn leaves_supported_roles_and_invalid_bodies_unchanged() {
        let supported = br#"{"messages":[{"role":"system","content":"be concise"}]}"#;
        assert_eq!(normalize_chat_request_for_native(supported), supported);

        let invalid = br#"not json"#;
        assert_eq!(normalize_chat_request_for_native(invalid), invalid);
    }
}
