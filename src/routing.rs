use crate::db::{EndpointApiKeySelection, ModelRouteCandidate, ModelRouteCandidateTarget};
use crate::response_affinity::{ResponseAffinityBinding, api_key_fingerprint};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundBindingState {
    Active,
    StaleEndpoint,
    StaleKey,
}

pub fn candidate_target_by_endpoint<'a>(
    candidate: &'a ModelRouteCandidate,
    endpoint_id: uuid::Uuid,
) -> Option<&'a ModelRouteCandidateTarget> {
    candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id == endpoint_id)
}

pub fn bound_binding_state(
    candidate: &ModelRouteCandidate,
    binding: &ResponseAffinityBinding,
) -> BoundBindingState {
    let Some(target) = candidate_target_by_endpoint(candidate, binding.endpoint_id) else {
        return BoundBindingState::StaleEndpoint;
    };
    if !target.enabled {
        return BoundBindingState::StaleEndpoint;
    }
    match select_bound_api_key(target, binding) {
        Some(_) => BoundBindingState::Active,
        None => BoundBindingState::StaleKey,
    }
}

pub fn select_bound_api_key(
    target: &ModelRouteCandidateTarget,
    binding: &ResponseAffinityBinding,
) -> Option<EndpointApiKeySelection> {
    let by_key_id = binding.endpoint_key_id.and_then(|key_id| {
        target.api_keys.iter().find(|key| {
            key.endpoint_id == target.endpoint_id
                && key.enabled
                && !key.api_key.trim().is_empty()
                && key.key_id == key_id
        })
    });
    let by_fingerprint = || {
        target.api_keys.iter().find(|key| {
            key.endpoint_id == target.endpoint_id
                && key.enabled
                && !key.api_key.trim().is_empty()
                && api_key_fingerprint(&key.api_key) == binding.endpoint_key_fingerprint
        })
    };
    let selected = by_key_id.or_else(by_fingerprint);
    selected
        .map(|key| EndpointApiKeySelection {
            key_id: (!key.key_id.is_nil()).then_some(key.key_id),
            key_label: (!key.key_id.is_nil()).then(|| key.key_label.clone()),
            secret: key.api_key.clone(),
        })
        .or_else(|| {
            (binding.endpoint_key_id.is_none()
                && api_key_fingerprint(&target.api_key) == binding.endpoint_key_fingerprint)
                .then(|| EndpointApiKeySelection {
                    key_id: None,
                    key_label: None,
                    secret: target.api_key.clone(),
                })
        })
}

/// Admin "preferred endpoint" preview: an equal-weight draw over the
/// candidate targets through the same unified-pool algorithm the runtime
/// uses. The quota cache is not available on this path, so every target
/// carries weight 1.0.
pub fn choose_preferred_target(
    candidate: &ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Option<ModelRouteCandidateTarget> {
    let entries = candidate
        .targets
        .iter()
        .map(|target| (target.target_id, 1.0_f64))
        .collect::<Vec<_>>();
    let index = unified_pool_draw(&entries, routing_key.unwrap_or("default"))?;
    candidate.targets.get(index).cloned()
}

/// Deterministic weighted draw over a unified key pool.
///
/// `entries` pairs every eligible unit with its stable unit id (an
/// `endpoint_api_keys.key_id`, or the target id for a secret-only unit)
/// and its non-negative weight; non-positive or non-finite weights are
/// ineligible. The digest is salted with `unified-key-pool` and the stable
/// routing key, then folded over the sorted unit ids. `endpoint_id` is
/// deliberately absent from the hash: units move between endpoints without
/// reshuffling the draw. Selection then scans the cumulative weights
/// exactly like the previous per-endpoint quota draw, so callers keep the
/// same bucket -> point -> cumulative algorithm.
pub fn unified_pool_draw(entries: &[(uuid::Uuid, f64)], stable_key: &str) -> Option<usize> {
    let mut eligible = entries
        .iter()
        .enumerate()
        .filter(|(_, (_, weight))| weight.is_finite() && *weight > 0.0)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return None;
    }
    eligible.sort_by_key(|index| entries[*index].0);
    let total = eligible.iter().map(|index| entries[*index].1).sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"unified-key-pool");
    hasher.update(stable_key.as_bytes());
    for index in &eligible {
        hasher.update(entries[*index].0.as_bytes());
    }
    let digest = hasher.finalize();
    let bucket = u64::from_be_bytes(digest[..8].try_into().expect("sha256 has eight bytes"));
    let point = (bucket as f64 / u64::MAX as f64) * total;
    let mut cumulative = 0.0_f64;
    let mut last = eligible[0];
    for index in eligible {
        cumulative += entries[index].1;
        last = index;
        if point < cumulative {
            return Some(index);
        }
    }
    Some(last)
}

pub fn stable_candidate_order<T, Score, TieBreak>(
    candidates: &[T],
    mut score: Score,
    mut tie_break: TieBreak,
) -> Vec<usize>
where
    Score: FnMut(usize, &T) -> [u8; 32],
    TieBreak: FnMut(usize, &T, usize, &T) -> Ordering,
{
    let mut indices: Vec<usize> = (0..candidates.len()).collect();
    indices.sort_by(|left, right| {
        score(*right, &candidates[*right])
            .cmp(&score(*left, &candidates[*left]))
            .then_with(|| tie_break(*left, &candidates[*left], *right, &candidates[*right]))
    });
    indices
}
