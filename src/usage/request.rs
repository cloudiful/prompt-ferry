use super::*;

pub fn model_from_body(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub fn rewrite_model_in_body(body: &[u8], model: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(object) = value.as_object_mut() else {
        return body.to_vec();
    };
    object.insert("model".to_string(), Value::String(model.to_string()));
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

pub fn upstream_body(path: &str, body: &[u8]) -> Vec<u8> {
    if path != "/v1/chat/completions" {
        return body.to_vec();
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(object) = value.as_object_mut() else {
        return body.to_vec();
    };
    if object.get("stream").and_then(Value::as_bool) != Some(true) {
        return body.to_vec();
    }
    let stream_options = object
        .entry("stream_options")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(stream_options) = stream_options.as_object_mut() else {
        return body.to_vec();
    };
    stream_options.insert("include_usage".to_string(), Value::Bool(true));
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

pub fn extract_request_prompt(path: &str, body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let text = match path {
        "/v1/chat/completions" => value
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .filter_map(|message| {
                        let role = message
                            .get("role")
                            .and_then(Value::as_str)
                            .unwrap_or("message");
                        let content = text::value_text(message.get("content")?);
                        (!content.is_empty()).then(|| format!("{role}: {content}"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        "/v1/responses" => text::value_text(value.get("input")?),
        _ => String::new(),
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}
