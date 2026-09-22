// Issue #384 Phase 4: upstream redaction moved from
// `prompt_ferry::redact_upstream`. Depends on the Phase 2 redact crate and
// the Phase 3 runtime-env envelope types; the root crate re-exports this
// crate so `prompt_ferry::redact_upstream::*` paths are unchanged.
use anyhow::Result;
use prompt_ferry_redact::{policy_generation, redactor_snapshot_for_user};
use prompt_ferry_runtime_env::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};
use redactor::{
    InputKind, RedactionSession, RedactorError, RestoreResult, RestoreState, SessionRedactor,
    ensure_restore_valid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Session budget ceilings. A restore state that exceeds any of these is not
/// persisted; the caller degrades to irreversible one-shot redaction.
const MAX_ENTRIES: usize = 2000;
const MAX_PERMITS: usize = 500;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpstreamRedactionSession {
    pub restore_state: RestoreState,
    /// Policy generation these tokens were minted under. `0` is the
    /// deserialization default, so sessions persisted before Issue #524 Task 5
    /// never match a live generation and are rebuilt.
    #[serde(default)]
    pub policy_generation: u64,
}

#[derive(Debug, Clone, Default)]
pub struct UpstreamRedactedRequest {
    pub body: Vec<u8>,
    pub redacted_request_json: Option<Value>,
    pub restore_session: Option<UpstreamRedactionSession>,
}

impl UpstreamRedactionSession {
    /// State stamped with the running policy generation, for callers that build
    /// session state directly instead of going through
    /// [`UpstreamRedactionProcessor`].
    pub fn current(restore_state: RestoreState) -> Self {
        Self {
            restore_state,
            policy_generation: policy_generation(),
        }
    }

    pub fn request_session(&self) -> &RedactionSession {
        self.restore_state.session()
    }

    /// Persisted `BIGINT` form of [`Self::policy_generation`].
    pub fn policy_version(&self) -> i64 {
        i64::try_from(self.policy_generation).unwrap_or(i64::MAX)
    }

    fn exceeds_budget(&self) -> bool {
        let started = std::time::Instant::now();
        let (exceeds, entries) = self.exceeds_budget_with_entries();
        if let Some(elapsed_us) =
            timing_sample(started.elapsed().as_micros() as u64, &BUDGET_CHECKS)
        {
            tracing::debug!(
                path = "budget",
                elapsed_us,
                entries,
                has_session = true,
                "redaction path timing"
            );
        }
        exceeds
    }

    fn exceeds_budget_with_entries(&self) -> (bool, usize) {
        let entries = self.restore_state.session().entries.len();
        if entries > MAX_ENTRIES {
            return (true, entries);
        }
        if self.restore_state.permits().len() > MAX_PERMITS {
            return (true, entries);
        }
        let serialized_len = serde_json::to_vec(self)
            .map(|serialized| serialized.len())
            .unwrap_or(0);
        (serialized_len > MAX_BYTES, entries)
    }
}

static BUDGET_CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Issue #528 Task 2: same sampled critical-path timing as the root crate's
/// `redaction_timing` module, inlined because the workspace root crate cannot
/// be a dependency here.
mod timing {
    use std::sync::atomic::{AtomicU64, Ordering};

    const SLOW_US: u64 = 10_000;
    const SAMPLE_EVERY: u64 = 100;

    pub(super) fn timing_sample(elapsed_us: u64, counter: &AtomicU64) -> Option<u64> {
        let call = counter.fetch_add(1, Ordering::Relaxed);
        (elapsed_us > SLOW_US || call.is_multiple_of(SAMPLE_EVERY)).then_some(elapsed_us)
    }
}
use timing::timing_sample;

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
    policy_generation: u64,
}

impl UpstreamRedactionProcessor {
    pub fn new(
        user_id: Option<i64>,
        external_id: Option<&str>,
        prior: Option<&UpstreamRedactionSession>,
    ) -> Result<Self, RedactorError> {
        let redactor = redactor_snapshot_for_user(user_id).ok_or_else(|| {
            RedactorError::Validation("redaction is disabled for this user".to_string())
        })?;
        // Single-request snapshot: the generation and the redactor are read
        // once, so a config change mid-request cannot split the session state.
        let generation = policy_generation();
        let prior = prior.filter(|session| session.policy_generation == generation);
        let prior_session = prior.map(UpstreamRedactionSession::request_session);
        let session = SessionRedactor::with_prior_session(prior_session, external_id)?;
        Ok(Self {
            redactor,
            session,
            prior_state: prior.map(|value| value.restore_state.clone()),
            policy_generation: generation,
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

    /// Prior-session entry count carried into this request (`0` without a
    /// prior session). Issue #528 Task 2 observation field; the in-flight
    /// entry count is only materialized in `finish_session`.
    pub fn prior_entry_count(&self) -> usize {
        self.prior_state
            .as_ref()
            .map(|state| state.session().entries.len())
            .unwrap_or(0)
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
        let session = UpstreamRedactionSession {
            restore_state,
            policy_generation: self.policy_generation,
        };
        if session.exceeds_budget() {
            return Ok(None);
        }
        Ok(Some(session))
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

/// Persisted `BIGINT` form of the running policy generation, used to gate
/// `conversation_redaction_sessions.policy_version` reads.
pub fn current_policy_version() -> i64 {
    i64::try_from(policy_generation()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
