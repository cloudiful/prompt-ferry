use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::redact_upstream::UpstreamRedactionSession;
use crate::worker::runtime::json_walker::walk_json_strings;
use redactor::ensure_restore_valid;

use super::upstream_text_fields::should_process_ai_string_field;

pub(crate) fn restore_ai_response_json(
    path: &str,
    body: &[u8],
    session: &UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body)?;
    let context = session.restore_state.restore_context()?;
    walk_json_strings(&mut value, |context_info, text| {
        let field_name = context_info.field_name.unwrap_or_default();
        if !should_process_ai_string_field(
            path,
            context_info.json_path,
            context_info.object_type,
            field_name,
        ) {
            return Ok(None);
        }
        restore_string(text, &context).map(Some)
    })?;
    Ok(serde_json::to_vec(&value)?)
}

pub(crate) fn restore_mcp_body_json(
    body: &[u8],
    session: &UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body)?;
    let context = session.restore_state.restore_context()?;
    walk_json_strings(&mut value, |_, text| {
        restore_string(text, &context).map(Some)
    })?;
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

fn restore_string(text: &str, context: &redactor::RestoreContext<'_>) -> Result<String> {
    let restored = context.restore_text(text);
    ensure_restore_valid(&restored).map_err(|err| anyhow!(err))?;
    Ok(restored.restored_text)
}
