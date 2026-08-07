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

pub fn choose_preferred_target(
    candidate: &ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Option<ModelRouteCandidateTarget> {
    rendezvous_target(candidate, routing_key).cloned()
}

pub fn ordered_route_targets(
    candidate: &ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Vec<ModelRouteCandidateTarget> {
    stable_candidate_order(
        &candidate.targets,
        |_, target| {
            rendezvous_score(
                routing_key.unwrap_or("default"),
                candidate.rule_id,
                target.endpoint_id,
            )
        },
        |_, left, _, right| left.position.cmp(&right.position),
    )
    .into_iter()
    .map(|index| candidate.targets[index].clone())
    .collect()
}

pub fn rendezvous_target<'a>(
    candidate: &'a ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Option<&'a ModelRouteCandidateTarget> {
    let routing_key = routing_key.unwrap_or("default");
    candidate
        .targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            (
                index,
                target,
                rendezvous_score(routing_key, candidate.rule_id, target.endpoint_id),
            )
        })
        .max_by(
            |(left_index, left, left_score), (right_index, right, right_score)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| right.position.cmp(&left.position))
                    .then_with(|| right_index.cmp(left_index))
            },
        )
        .map(|(_, target, _)| target)
}

pub fn rendezvous_score(
    routing_key: &str,
    rule_id: uuid::Uuid,
    endpoint_id: uuid::Uuid,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(routing_key.as_bytes());
    hasher.update(rule_id.as_bytes());
    hasher.update(endpoint_id.as_bytes());
    hasher.finalize().into()
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
