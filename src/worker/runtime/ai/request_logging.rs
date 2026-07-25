use crate::{
    db,
    upstream_adapter::{PreparedRequestBody, PreparedUpstreamRequest},
};
use serde_json::Value;
use tracing::info;

pub(super) fn log_prepared_upstream_summary(
    route: &db::RouteConfig,
    prepared: &PreparedUpstreamRequest,
) {
    let body = match &prepared.body {
        PreparedRequestBody::PassthroughStream(bytes)
        | PreparedRequestBody::BufferedBytes(bytes) => bytes.as_slice(),
    };
    let (assistant_tool_call_count, reasoning_field_count) =
        prepared_request_summary(body).unwrap_or_default();
    info!(
        endpoint_id = %route.route_id,
        upstream_path = %prepared.path,
        adapter = ?prepared.response_adapter,
        assistant_tool_call_count,
        reasoning_field_count,
        "prepared upstream request summary"
    );
}

fn prepared_request_summary(body: &[u8]) -> Option<(usize, usize)> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let mut assistant_tool_call_count = 0;
    let mut reasoning_field_count = 0;
    for items in [
        value.get("messages").and_then(Value::as_array),
        value.get("input").and_then(Value::as_array),
    ]
    .into_iter()
    .flatten()
    {
        for item in items {
            let Some(object) = item.as_object() else {
                continue;
            };
            match object.get("type").and_then(Value::as_str) {
                Some("function_call") => assistant_tool_call_count += 1,
                Some("reasoning") => reasoning_field_count += 1,
                _ => {}
            }
            if object.get("role").and_then(Value::as_str) == Some("assistant") {
                assistant_tool_call_count += object
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                reasoning_field_count += ["reasoning_content", "reasoning_details"]
                    .into_iter()
                    .filter(|field| object.get(*field).is_some_and(has_meaningful_value))
                    .count();
            }
        }
    }
    Some((assistant_tool_call_count, reasoning_field_count))
}

fn has_meaningful_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(object) => !object.is_empty(),
        Value::Number(_) => true,
    }
}
