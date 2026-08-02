use anyhow::Result;
use redactor::{
    InputKind, RedactionSession, RedactorError, RestoreResult, RestoreState, SessionRedactor,
    ensure_restore_valid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    redact,
    relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpstreamRedactionSession {
    pub restore_state: RestoreState,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UpstreamRedactedRequest {
    pub(crate) body: Vec<u8>,
    pub(crate) redacted_request_json: Option<Value>,
    pub(crate) restore_session: Option<UpstreamRedactionSession>,
}

impl UpstreamRedactionSession {
    pub fn request_session(&self) -> &RedactionSession {
        self.restore_state.session()
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpstreamRedactionResult {
    pub redacted_text: String,
    pub session: Option<UpstreamRedactionSession>,
    pub applied: bool,
}

pub struct UpstreamRedactionProcessor {
    redactor: redactor::Redactor,
    session: SessionRedactor,
    prior_state: Option<RestoreState>,
}

impl UpstreamRedactionProcessor {
    pub fn new(
        user_id: Option<i64>,
        external_id: Option<&str>,
        prior: Option<&UpstreamRedactionSession>,
    ) -> Result<Self, RedactorError> {
        let redactor = redact::redactor_snapshot_for_user(user_id).ok_or_else(|| {
            RedactorError::Validation("redaction is disabled for this user".to_string())
        })?;
        let prior_session = prior.map(UpstreamRedactionSession::request_session);
        let session = SessionRedactor::with_prior_session(prior_session, external_id)?;
        Ok(Self {
            redactor,
            session,
            prior_state: prior.map(|value| value.restore_state.clone()),
        })
    }

    pub fn redact_fragment(
        &mut self,
        text: &str,
        input_kind: InputKind,
    ) -> Result<String, RedactorError> {
        self.session
            .redact_fragment_with_input_kind(&self.redactor, text, input_kind)
    }

    pub fn has_applied_replacements(&self) -> bool {
        self.session.has_applied_replacements()
    }

    pub fn finish_state(
        &self,
        original_text: &str,
        redacted_text: &str,
    ) -> Result<Option<UpstreamRedactionSession>, RedactorError> {
        if self.prior_state.is_none() && !self.has_applied_replacements() {
            return Ok(None);
        }
        let request_session =
            self.session
                .finish_session(original_text, redacted_text, self.redactor.policy());
        let restore_state = match &self.prior_state {
            Some(prior) => prior.advance(request_session),
            None => RestoreState::new(request_session),
        }
        .map_err(|err| RedactorError::Validation(err.to_string()))?;
        Ok(Some(UpstreamRedactionSession { restore_state }))
    }
}

pub fn encrypt_upstream_session(
    manager: &RelaySecretManager,
    session: &UpstreamRedactionSession,
) -> Result<EncryptedSecretEnvelope> {
    let serialized = serde_json::to_string(session)?;
    manager.encrypt(&serialized)
}

pub fn decrypt_upstream_session(
    manager: &RelaySecretManager,
    envelope: &EncryptedSecretEnvelope,
) -> Result<UpstreamRedactionSession> {
    let plaintext = manager.decrypt(envelope)?;
    Ok(serde_json::from_str(&plaintext)?)
}

pub fn redact_text_with_stateful_session(
    text: &str,
    input_kind: InputKind,
    user_id: Option<i64>,
    external_id: Option<&str>,
    prior: Option<&UpstreamRedactionSession>,
) -> Result<UpstreamRedactionResult, RedactorError> {
    let mut processor = UpstreamRedactionProcessor::new(user_id, external_id, prior)?;
    let redacted_text = processor.redact_fragment(text, input_kind)?;
    let applied = processor.has_applied_replacements();
    let session = processor.finish_state(text, &redacted_text)?;
    Ok(UpstreamRedactionResult {
        redacted_text,
        applied,
        session,
    })
}

pub fn restore_text(text: &str, session: &UpstreamRedactionSession) -> Result<RestoreResult> {
    let context = session.restore_state.restore_context()?;
    let restored = context.restore_text(text);
    ensure_restore_valid(&restored)?;
    Ok(restored)
}

#[cfg(test)]
mod tests;
