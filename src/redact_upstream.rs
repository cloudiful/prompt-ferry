use anyhow::{Result, anyhow};
use redactor::{
    InputKind, RedactionSession, RedactorError, RestoreContext, RestoreResult, RestoreState,
    SessionRedactor,
};
use serde::{Deserialize, Serialize};

use crate::{
    redact,
    relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpstreamRedactionSession {
    pub restore_state: RestoreState,
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
        let session = SessionRedactor::with_prior_session(prior_session, external_id).or_else(
            |prior_error| {
                if prior_session.is_some() {
                    SessionRedactor::with_prior_session(None, external_id).or(Err(prior_error))
                } else {
                    Err(prior_error)
                }
            },
        )?;
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

pub struct UpstreamRestoreContext<'a> {
    context: RestoreContext<'a>,
}

impl<'a> UpstreamRestoreContext<'a> {
    pub fn new(session: &'a UpstreamRedactionSession) -> Result<Self> {
        Ok(Self {
            context: session.restore_state.restore_context()?,
        })
    }

    pub fn restore_text(&self, text: &str) -> RestoreResult {
        self.context.restore_text(text)
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
) -> UpstreamRedactionResult {
    let mut processor = match UpstreamRedactionProcessor::new(user_id, external_id, prior) {
        Ok(processor) => processor,
        Err(_) => {
            return UpstreamRedactionResult {
                redacted_text: text.to_string(),
                session: None,
                applied: false,
            };
        }
    };
    let redacted_text = match processor.redact_fragment(text, input_kind) {
        Ok(redacted_text) => redacted_text,
        Err(_) => {
            return UpstreamRedactionResult {
                redacted_text: text.to_string(),
                session: None,
                applied: false,
            };
        }
    };
    let applied = processor.has_applied_replacements();
    let session = match processor.finish_state(text, &redacted_text) {
        Ok(session) => session,
        Err(_) => {
            return UpstreamRedactionResult {
                redacted_text: text.to_string(),
                session: prior.cloned(),
                applied: false,
            };
        }
    };
    UpstreamRedactionResult {
        redacted_text,
        applied,
        session,
    }
}

pub fn restore_text(text: &str, session: &UpstreamRedactionSession) -> Result<RestoreResult> {
    Ok(UpstreamRestoreContext::new(session)?.restore_text(text))
}

pub fn envelope_from_session(
    manager: &RelaySecretManager,
    session: Option<&UpstreamRedactionSession>,
) -> Result<crate::db::EncryptedPayloadInput> {
    let Some(session) = session else {
        return Ok(crate::db::EncryptedPayloadInput::default());
    };
    let encrypted = encrypt_upstream_session(manager, session)?;
    Ok(crate::db::EncryptedPayloadInput {
        ciphertext: Some(encrypted.ciphertext),
        nonce: Some(encrypted.nonce),
        key_version: Some(encrypted.key_version),
    })
}

pub fn envelope_to_session(
    manager: &RelaySecretManager,
    envelope: &crate::db::EncryptedPayloadInput,
) -> Result<Option<UpstreamRedactionSession>> {
    match (
        envelope.ciphertext.as_ref(),
        envelope.nonce.as_ref(),
        envelope.key_version,
    ) {
        (Some(ciphertext), Some(nonce), Some(key_version)) => decrypt_upstream_session(
            manager,
            &EncryptedSecretEnvelope {
                ciphertext: ciphertext.clone(),
                nonce: nonce.clone(),
                key_version,
            },
        )
        .map(Some),
        (None, None, None) => Ok(None),
        _ => Err(anyhow!("incomplete upstream restore session envelope")),
    }
}

#[cfg(test)]
mod tests;
