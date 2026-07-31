use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub(crate) fn tool_calls_match(current: &Value, artifact: &Value) -> bool {
    tool_call_signatures(current) == tool_call_signatures(artifact)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolCallArguments {
    Json(Value),
    Raw(String),
}

fn tool_call_signatures(value: &Value) -> Option<HashMap<String, (String, ToolCallArguments)>> {
    let tool_calls = value.get("tool_calls").and_then(Value::as_array)?;
    let mut signatures = HashMap::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let object = tool_call.as_object()?;
        let id = object.get("id").and_then(Value::as_str)?.to_string();
        let function = object.get("function").and_then(Value::as_object)?;
        let name = function.get("name").and_then(Value::as_str)?.to_string();
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let arguments = match serde_json::from_str::<Value>(&arguments) {
            Ok(value) => ToolCallArguments::Json(value),
            Err(_) => ToolCallArguments::Raw(arguments),
        };
        if signatures.insert(id, (name, arguments)).is_some() {
            return None;
        }
    }
    Some(signatures)
}

pub(super) fn signature_hash(value: &Value) -> Option<String> {
    let signatures = tool_call_signatures(value)?;
    let mut canonical = signatures
        .into_iter()
        .map(|(id, (name, arguments))| {
            let arguments = match arguments {
                ToolCallArguments::Json(value) => value,
                ToolCallArguments::Raw(value) => Value::String(value),
            };
            json!({"id": id, "name": name, "arguments": arguments})
        })
        .collect::<Vec<_>>();
    canonical.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let bytes = serde_json::to_vec(&canonical).ok()?;
    Some(
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}
