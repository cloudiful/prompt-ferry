use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::warn;

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
mod tests {
    use redactor::{FindingKind, InputKind, RedactionPolicy, RedactorBuilder, RestoreState};

    use super::{restore_ai_response_json, restore_mcp_body_json};
    use crate::redact_upstream::UpstreamRedactionSession;

    fn session(original: &str) -> (UpstreamRedactionSession, String) {
        let redactor = RedactorBuilder::new()
            .with_redaction_policy(RedactionPolicy::default().with_kind(FindingKind::Domain, true))
            .build();
        let artifact = redactor
            .redact_artifact_with_input_kind_source_and_prior_session(
                original,
                InputKind::Text,
                None,
                None,
                Some("conversation"),
            )
            .expect("redact");
        let token = artifact.session.issued_tokens[0].clone();
        (
            UpstreamRedactionSession {
                restore_state: RestoreState::new(artifact.session).expect("state"),
            },
            token,
        )
    }

    #[test]
    fn restores_valid_tokens_and_preserves_invalid_ai_tokens() {
        let (session, token) = session("a.example.com");
        let text = format!(
            "valid {token} malformed [[RDX:v2:...]] unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
        );
        let body = serde_json::json!({
            "output": [{"type": "output_text", "text": text}]
        });

        let restored = restore_ai_response_json(
            "/v1/responses",
            &serde_json::to_vec(&body).expect("encode"),
            &session,
        )
        .expect("restore");
        let restored: serde_json::Value = serde_json::from_slice(&restored).expect("decode");

        assert_eq!(
            restored["output"][0]["text"],
            "valid a.example.com malformed [[RDX:v2:...]] unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
        );
    }

    #[test]
    fn invalid_ai_json_still_fails() {
        let (session, _) = session("a.example.com");
        assert!(restore_ai_response_json("/v1/responses", br#"{"#, &session).is_err());
    }

    #[test]
    fn mcp_restore_remains_strict_for_invalid_tokens() {
        let (session, _) = session("a.example.com");
        let body = br#"{"text":"[[RDX:v2:...]]"}"#;

        assert!(restore_mcp_body_json(body, &session).is_err());
    }
}
