use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::worker_admin_types::{
    CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
};

/// Observation providers accepted by storage, mirroring the provider enum the
/// database enforces. Collection capability is decided elsewhere; storage
/// only guarantees the value is a known provider identity, never a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaObservationProvider {
    Generic,
    Minimax,
    CommandCode,
    OpencodeGo,
    Openrouter,
    Glm,
    Deepseek,
    Openai,
}

impl QuotaObservationProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Minimax => "minimax",
            Self::CommandCode => "command_code",
            Self::OpencodeGo => "opencode_go",
            Self::Openrouter => "openrouter",
            Self::Glm => "glm",
            Self::Deepseek => "deepseek",
            Self::Openai => "openai",
        }
    }

    pub(crate) fn from_database(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(Self::Generic),
            "minimax" => Some(Self::Minimax),
            "command_code" => Some(Self::CommandCode),
            "opencode_go" => Some(Self::OpencodeGo),
            "openrouter" => Some(Self::Openrouter),
            "glm" => Some(Self::Glm),
            "deepseek" => Some(Self::Deepseek),
            "openai" => Some(Self::Openai),
            _ => None,
        }
    }
}

/// Observation source: the fetch path that produced the observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaObservationSource {
    Manual,
    Request,
    Periodic,
}

impl QuotaObservationSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Request => "request",
            Self::Periodic => "periodic",
        }
    }

    pub(crate) fn from_database(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "request" => Some(Self::Request),
            "periodic" => Some(Self::Periodic),
            _ => None,
        }
    }
}

pub(crate) const MAX_OBSERVATION_WINDOWS: usize = 64;
pub(crate) const MAX_OBSERVATION_IDENTIFIER_CHARS: usize = 128;
pub(crate) const MAX_KEY_LABEL_CHARS: usize = 200;
pub(crate) const MAX_ERROR_CODE_CHARS: usize = 64;
pub(crate) const MAX_PLAN_TYPE_CHARS: usize = 128;

/// One generic per-key quota observation to persist. The windows payload is
/// the existing `CanonicalQuotaWindow` Rust type, so only its allowlisted
/// fields can reach storage; use [`QuotaObservationCreate::new`] so every
/// field is validated before the first write.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaObservationCreate {
    pub endpoint_id: Uuid,
    pub key_id: Uuid,
    pub provider: QuotaObservationProvider,
    pub key_label: String,
    pub source: QuotaObservationSource,
    pub observed_at: DateTime<Utc>,
    pub ok: bool,
    pub status: Option<String>,
    pub error_code: Option<String>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Vec<CanonicalQuotaWindow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuotaObservation {
    pub observation_id: i64,
    pub endpoint_id: Uuid,
    pub key_id: Uuid,
    pub provider: QuotaObservationProvider,
    pub key_label: String,
    pub source: QuotaObservationSource,
    pub observed_at: DateTime<Utc>,
    pub ok: bool,
    pub status: Option<String>,
    pub error_code: Option<String>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Vec<CanonicalQuotaWindow>,
    pub created_at: DateTime<Utc>,
}

impl QuotaObservationCreate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint_id: Uuid,
        key_id: Uuid,
        provider: QuotaObservationProvider,
        key_label: String,
        source: QuotaObservationSource,
        observed_at: DateTime<Utc>,
        ok: bool,
        status: Option<String>,
        error_code: Option<String>,
        plan_type: Option<String>,
        limit_reached: Option<bool>,
        windows: Vec<CanonicalQuotaWindow>,
    ) -> Result<Self> {
        let create = Self {
            endpoint_id,
            key_id,
            provider,
            key_label,
            source,
            observed_at,
            ok,
            status,
            error_code,
            plan_type,
            limit_reached,
            windows,
        };
        create.validate()?;
        Ok(create)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.key_label.chars().count() > MAX_KEY_LABEL_CHARS {
            bail!("quota observation key_label exceeds {MAX_KEY_LABEL_CHARS} characters");
        }
        if let Some(error_code) = self.error_code.as_deref() {
            if error_code.is_empty()
                || error_code.chars().count() > MAX_ERROR_CODE_CHARS
                || !error_code.chars().all(|char| {
                    char.is_ascii_lowercase()
                        || char.is_ascii_digit()
                        || matches!(char, '_' | ':' | '-')
                })
            {
                bail!(
                    "quota observation error_code must be a stable code of 1..={MAX_ERROR_CODE_CHARS} [a-z0-9:_-]"
                );
            }
        }
        if let Some(plan_type) = self.plan_type.as_deref() {
            if plan_type.is_empty() || plan_type.chars().count() > MAX_PLAN_TYPE_CHARS {
                bail!("quota observation plan_type must be 1..={MAX_PLAN_TYPE_CHARS} characters");
            }
        }
        if !self.ok && !self.windows.is_empty() {
            bail!("failed quota observations must carry empty windows");
        }
        if self.windows.len() > MAX_OBSERVATION_WINDOWS {
            bail!("quota observations carry at most {MAX_OBSERVATION_WINDOWS} windows");
        }
        for window in &self.windows {
            validate_canonical_window(window)?;
        }
        Ok(())
    }
}

/// Canonical window rules for persisted observations: identifiers are
/// preserved as reported (never normalized or converted), every present
/// number is finite, percentages stay in 0..=100, amount-only kinds and
/// unknown availability never carry a synthetic percentage, and no secret
/// field exists on the type so none can leak into storage.
pub(crate) fn validate_canonical_window(window: &CanonicalQuotaWindow) -> Result<()> {
    let source_chars = window.source_id.chars().count();
    if source_chars == 0 || source_chars > MAX_OBSERVATION_IDENTIFIER_CHARS {
        bail!("quota window source_id must be 1..={MAX_OBSERVATION_IDENTIFIER_CHARS} characters");
    }
    if window
        .source_model
        .as_deref()
        .is_some_and(|model| model.chars().count() > MAX_OBSERVATION_IDENTIFIER_CHARS)
    {
        bail!("quota window source_model exceeds {MAX_OBSERVATION_IDENTIFIER_CHARS} characters");
    }
    if window.unit.as_deref().is_some_and(str::is_empty) {
        bail!("quota window unit must not be empty when present");
    }
    if window.duration_seconds.is_some_and(|seconds| seconds <= 0) {
        bail!("quota window duration_seconds must be positive when present");
    }
    for (field, value) in [
        ("remaining_percent", window.remaining_percent),
        ("current", window.current),
        ("limit", window.limit),
        ("remaining", window.remaining),
    ] {
        if value.is_some_and(|number| !number.is_finite()) {
            bail!("quota window {field} must be finite when present");
        }
    }
    if window
        .remaining_percent
        .is_some_and(|percent| !(0.0..=100.0).contains(&percent))
    {
        bail!("quota window remaining_percent must be between 0 and 100");
    }
    if matches!(
        window.kind,
        CanonicalQuotaWindowKind::Balance | CanonicalQuotaWindowKind::Unlimited
    ) && window.remaining_percent.is_some()
    {
        bail!("balance and unlimited quota windows never carry a percentage");
    }
    if matches!(window.availability, SubscriptionWindowAvailability::Unknown)
        && window.remaining_percent.is_some()
    {
        bail!("unknown quota windows never carry a percentage");
    }
    Ok(())
}
