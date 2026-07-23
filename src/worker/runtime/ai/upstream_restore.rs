use anyhow::{Result, anyhow};
use serde_json::{Map, Value};

use crate::redact_upstream::{UpstreamRedactionSession, UpstreamRestoreContext};

use super::upstream_text_fields::should_process_ai_string_field;

pub(crate) fn restore_ai_response_json(
    path: &str,
    body: &[u8],
    session: &UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body)?;
    let context = UpstreamRestoreContext::new(session)?;
    restore_value(path, "", &mut value, &context)?;
    Ok(serde_json::to_vec(&value)?)
}

pub(crate) fn restore_mcp_body_json(
    body: &[u8],
    session: &UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body)?;
    let context = UpstreamRestoreContext::new(session)?;
    restore_generic_text_values(&mut value, &context)?;
    Ok(serde_json::to_vec(&value)?)
}

pub(crate) async fn restore_ai_response_json_blocking(
    path: String,
    body: Vec<u8>,
    session: UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || restore_ai_response_json(&path, &body, &session))
        .await
        .map_err(|err| anyhow!("AI response restore task failed: {err}"))?
}

pub(crate) async fn restore_mcp_body_json_blocking(
    body: Vec<u8>,
    session: UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || restore_mcp_body_json(&body, &session))
        .await
        .map_err(|err| anyhow!("MCP response restore task failed: {err}"))?
}

fn restore_value(
    request_path: &str,
    json_path: &str,
    value: &mut Value,
    context: &UpstreamRestoreContext<'_>,
) -> Result<()> {
    match value {
        Value::Object(object) => restore_object(request_path, json_path, object, context),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                restore_value(request_path, &format!("{json_path}/{index}"), item, context)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn restore_object(
    request_path: &str,
    json_path: &str,
    object: &mut Map<String, Value>,
    context: &UpstreamRestoreContext<'_>,
) -> Result<()> {
    let object_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let keys = object.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(value) = object.get_mut(&key) else {
            continue;
        };
        let child_path = format!("{json_path}/{key}");
        let should_restore = should_restore_field(
            request_path,
            &child_path,
            object_type.as_deref(),
            &key,
            value,
        );
        match value {
            Value::String(text) if should_restore => restore_string(text, context)?,
            Value::Array(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    restore_value(
                        request_path,
                        &format!("{child_path}/{index}"),
                        item,
                        context,
                    )?;
                }
            }
            Value::Object(inner) => restore_object(request_path, &child_path, inner, context)?,
            _ => {}
        }
    }
    Ok(())
}

fn should_restore_field(
    request_path: &str,
    json_path: &str,
    object_type: Option<&str>,
    key: &str,
    value: &Value,
) -> bool {
    should_process_ai_string_field(request_path, json_path, object_type, key, value)
}

fn restore_generic_text_values(
    value: &mut Value,
    context: &UpstreamRestoreContext<'_>,
) -> Result<()> {
    match value {
        Value::String(text) => restore_string(text, context),
        Value::Array(items) => {
            for item in items {
                restore_generic_text_values(item, context)?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                restore_generic_text_values(value, context)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn restore_string(text: &mut String, context: &UpstreamRestoreContext<'_>) -> Result<()> {
    let restored = context.restore_text(text);
    if !restored.is_valid() {
        return Err(anyhow!(restored.validation_errors.join("; ")));
    }
    *text = restored.restored_text;
    Ok(())
}
