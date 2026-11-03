//! Quota-aware endpoint key selection for the builtin MiniMax tools
//! (issue #1179 P1).
//!
//! The candidates are the enabled, non-blank endpoint key rows. The tool
//! assesses every candidate against the shared quota cache and draws one
//! from the eligible ones with the same weighted pool draw the AI routes
//! use, so a key with an exhausted quota is never called while a funded key
//! is available.

use crate::worker_admin::token_plan_cache::{QuotaAssessment, QuotaScope, TokenPlanQuotaCache};
use std::collections::BTreeSet;
use uuid::Uuid;

/// One credential a builtin tool may call with.
#[derive(Clone, Debug)]
pub(super) struct Candidate {
    /// The endpoint key id. `None` for the legacy inline endpoint key,
    /// which has no key row and therefore no quota observation.
    pub key_id: Option<Uuid>,
    /// A non-secret label for diagnostics.
    pub label: String,
    /// The bearer secret. It never reaches a log line or an error message.
    pub secret: String,
    /// The endpoint key position, reported as the MCP token slot.
    pub position: i32,
}

/// The quota scope a builtin tool assesses.
///
/// `web_search` is billed against the general quota group, so the cached
/// general snapshot gates its candidates. The image tool bills a group the
/// provider does not document, so it never infers one: it spreads over the
/// enabled keys and reacts to the responses it actually receives.
#[derive(Clone, Copy, Debug)]
pub(super) struct ToolScope {
    pub model: Option<&'static str>,
    pub allow_general: bool,
    pub snapshot: bool,
}

pub(super) const WEB_SEARCH_SCOPE: ToolScope = ToolScope {
    model: Some("general"),
    allow_general: true,
    snapshot: true,
};

pub(super) const UNDERSTAND_IMAGE_SCOPE: ToolScope = ToolScope {
    model: None,
    allow_general: false,
    snapshot: false,
};

/// Why no candidate could be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PoolState {
    /// Every candidate was already tried in this call.
    Empty,
    /// Every remaining candidate reports an exhausted quota.
    Exhausted,
    /// Every remaining candidate is in a rate-limit cooldown.
    Cooling,
    /// Every remaining candidate is being probed by another request.
    ProbeBusy,
    /// The remaining candidates are blocked for different reasons.
    Blocked,
}

/// The drawn candidate.
#[derive(Clone, Copy, Debug)]
pub(super) struct Drawn {
    pub index: usize,
}

/// The outcome of one selection round.
#[derive(Clone, Copy, Debug)]
pub(super) enum Selection {
    Drawn(Drawn),
    Unavailable(PoolState),
}

/// The enabled, non-blank endpoint key rows, ordered by position.
///
/// A key row that is disabled or blank is never a candidate. The legacy inline
/// endpoint key is only used when the endpoint has no key row at all: when
/// rows exist but none qualifies, the endpoint has no usable credential and the
/// call must not send the inline secret (issue #1179 P2 repair — the ordinary
/// endpoint save derives that column from the first key row, even a disabled
/// one).
pub(super) fn candidates(endpoint: &crate::db::ProviderEndpoint) -> Vec<Candidate> {
    let mut candidates = endpoint
        .api_keys
        .iter()
        .filter(|key| key.enabled && !key.api_key.trim().is_empty())
        .map(|key| Candidate {
            key_id: Some(key.key_id),
            label: key.key_label.clone(),
            secret: key.api_key.clone(),
            position: key.position,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.position
            .cmp(&right.position)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    if candidates.is_empty() && endpoint.api_keys.is_empty() && !endpoint.api_key.trim().is_empty()
    {
        candidates.push(Candidate {
            key_id: None,
            label: "inline".to_string(),
            secret: endpoint.api_key.clone(),
            position: 0,
        });
    }
    candidates
}

/// Assess one candidate against the shared quota cache.
///
/// A tool whose billing group the provider does not document is never gated by
/// the shared model-specific feedback (issue #1179 P2 repair): the image tool
/// does not consult the shared assessment at all, so a web_search exhaustion,
/// rate cooldown or recovery probe on a key never removes that key from the
/// image pool. Its candidates keep neutral weights and the call reacts to the
/// responses it actually receives.
pub(super) fn assess(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint_id: Uuid,
    candidate: &Candidate,
    scope: ToolScope,
) -> QuotaAssessment {
    if !scope.snapshot {
        return QuotaAssessment::Eligible(1.0);
    }
    let Some(cache) = cache else {
        return QuotaAssessment::Eligible(1.0);
    };
    cache.assess_scope(QuotaScope::new(
        endpoint_id,
        candidate.key_id,
        scope.model,
        scope.allow_general,
    ))
}

/// Draw one eligible candidate.
///
/// The remaining candidates are reassessed on every round, so a key that
/// became exhausted, entered a cooldown or started a probe while the call
/// was running drops out before the next attempt.
pub(super) fn draw(
    cache: Option<&TokenPlanQuotaCache>,
    endpoint_id: Uuid,
    candidates: &[Candidate],
    scope: ToolScope,
    attempted: &BTreeSet<usize>,
    stable_key: &str,
) -> Selection {
    let mut eligible: Vec<(usize, f64)> = Vec::new();
    let mut remaining = 0usize;
    let mut exhausted = 0usize;
    let mut cooling = 0usize;
    let mut probe_busy = 0usize;
    for (index, candidate) in candidates.iter().enumerate() {
        if attempted.contains(&index) {
            continue;
        }
        remaining += 1;
        match assess(cache, endpoint_id, candidate, scope) {
            QuotaAssessment::Eligible(weight) if weight > 0.0 => eligible.push((index, weight)),
            QuotaAssessment::Eligible(_) => exhausted += 1,
            QuotaAssessment::Exhausted => exhausted += 1,
            QuotaAssessment::Cooling => cooling += 1,
            QuotaAssessment::ProbeBusy => probe_busy += 1,
        }
    }
    if eligible.is_empty() {
        let state = if remaining == 0 {
            PoolState::Empty
        } else if exhausted == remaining {
            PoolState::Exhausted
        } else if cooling == remaining {
            PoolState::Cooling
        } else if probe_busy == remaining {
            PoolState::ProbeBusy
        } else {
            PoolState::Blocked
        };
        return Selection::Unavailable(state);
    }
    let entries = eligible
        .iter()
        .map(|(index, weight)| (candidates[*index].key_id.unwrap_or(Uuid::nil()), *weight))
        .collect::<Vec<_>>();
    let Some(drawn) = crate::routing::unified_pool_draw(&entries, stable_key) else {
        return Selection::Unavailable(PoolState::Blocked);
    };
    let (index, _weight) = eligible[drawn];
    Selection::Drawn(Drawn { index })
}

/// The draw key of one tool call.
///
/// The conversation id keeps a weak affinity influence between the eligible
/// keys, while the per-call nonce keeps consecutive calls apart: a call
/// never pins one key, and an exhausted key is never drawn because it is not
/// in the pool at all.
pub(super) fn draw_key(conversation_id: Option<&str>, nonce: u64) -> String {
    match conversation_id {
        Some(conversation) => format!("minimax-mcp:{nonce}:{conversation}"),
        None => format!("minimax-mcp:{nonce}"),
    }
}

/// A fresh per-call nonce for the selection entropy.
pub(super) fn nonce() -> u64 {
    use rand::RngExt as _;
    rand::rng().random::<u64>()
}
