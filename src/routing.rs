use crate::db::{ModelRouteCandidate, ModelRouteCandidateTarget};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

pub fn choose_preferred_target(
    candidate: &ModelRouteCandidate,
    routing_key: Option<&str>,
) -> Option<ModelRouteCandidateTarget> {
    ordered_route_targets(candidate, routing_key)
        .into_iter()
        .next()
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
    .next()
    .and_then(|index| candidate.targets.get(index))
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
