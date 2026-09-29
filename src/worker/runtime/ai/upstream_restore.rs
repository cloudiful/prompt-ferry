use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::warn;

use super::upstream_text_fields::should_process_ai_string_field;
use crate::redact_upstream::UpstreamRedactionSession;
use crate::redaction_timing::{PATH_RESTORE, timing_sample};
use crate::worker::runtime::json_walker::walk_json_strings;
use redactor::ensure_restore_valid;

static RESTORE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn restore_ai_response_json(
    path: &str,
    body: &[u8],
    session: &UpstreamRedactionSession,
) -> Result<Vec<u8>> {
    let started = std::time::Instant::now();
    let outcome = restore_ai_response_json_inner(path, body, session);
    if let Some((elapsed_us, _)) =
        timing_sample(started.elapsed().as_micros() as u64, &RESTORE_CALLS)
    {
        tracing::debug!(
            path = PATH_RESTORE,
            elapsed_us,
            entries = session.restore_state.session().entries.len(),
            has_session = true,
            "redaction path timing"
        );
    }
    outcome
}

fn restore_ai_response_json_inner(
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
        restore_ai_string(text, &context).map(Some)
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
        restore_mcp_string(text, &context).map(Some)
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

pub(super) fn log_restore_diagnostics(result: &redactor::RestoreResult, surface: &'static str) {
    if result.skipped_tokens.is_empty()
        && result.validation_errors.is_empty()
        && result.unresolved_tokens.is_empty()
    {
        return;
    }

    warn!(
        restore_surface = surface,
        skipped_token_count = result.skipped_tokens.len(),
        validation_error_count = result.validation_errors.len(),
        unresolved_token_count = result.unresolved_tokens.len(),
        "passed through upstream redaction tokens during AI restore"
    );
}

fn restore_ai_string(text: &str, context: &redactor::RestoreContext<'_>) -> Result<String> {
    let restored = context.restore_text(text);
    log_restore_diagnostics(&restored, "ai_json");
    Ok(restored.restored_text)
}

fn restore_mcp_string(text: &str, context: &redactor::RestoreContext<'_>) -> Result<String> {
    let restored = context.restore_text(text);
    ensure_restore_valid(&restored).map_err(|err| anyhow!(err))?;
    Ok(restored.restored_text)
}

#[cfg(test)]
#[path = "upstream_restore_blocking_tests.rs"]
mod blocking_tests;
#[cfg(test)]
#[path = "upstream_restore_diagnostics_tests.rs"]
mod diagnostics_tests;
