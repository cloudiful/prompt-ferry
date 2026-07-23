use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::sync::Mutex;

use crate::routing::stable_candidate_order;

#[derive(Default)]
pub(super) struct McpBearerTokenBalancer {
    server_state: Mutex<HashMap<uuid::Uuid, Arc<Mutex<ServerTokenState>>>>,
}

#[derive(Debug, Default)]
struct ServerTokenState {
    tokens: Vec<String>,
    usage_counts: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectedToken {
    pub(super) value: Option<String>,
    pub(super) index: Option<usize>,
}

impl McpBearerTokenBalancer {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) async fn select_token(
        &self,
        server_id: uuid::Uuid,
        tokens: &[String],
        attempted: &[usize],
        conversation_id: Option<&str>,
    ) -> SelectedToken {
        if tokens.is_empty() {
            return SelectedToken {
                value: None,
                index: None,
            };
        }

        if let Some(index) = sticky_token_index(server_id, tokens, attempted, conversation_id) {
            return SelectedToken {
                value: Some(tokens[index].clone()),
                index: Some(index),
            };
        }

        let state = {
            let mut guard = self.server_state.lock().await;
            guard
                .entry(server_id)
                .or_insert_with(|| Arc::new(Mutex::new(ServerTokenState::default())))
                .clone()
        };

        let mut state = state.lock().await;
        if state.tokens != tokens {
            state.tokens = tokens.to_vec();
            state.usage_counts = vec![0; tokens.len()];
        } else if state.usage_counts.len() != tokens.len() {
            state.usage_counts.resize(tokens.len(), 0);
        }

        let attempted_set: HashSet<usize> = attempted.iter().copied().collect();
        let mut best_index = None;
        let mut best_count = u64::MAX;
        for (index, count) in state.usage_counts.iter().copied().enumerate() {
            if attempted_set.contains(&index) {
                continue;
            }
            if count < best_count {
                best_count = count;
                best_index = Some(index);
            }
        }

        let index = best_index.unwrap_or(0);
        state.usage_counts[index] = state.usage_counts[index].saturating_add(1);
        SelectedToken {
            value: Some(state.tokens[index].clone()),
            index: Some(index),
        }
    }
}

fn sticky_token_index(
    server_id: uuid::Uuid,
    tokens: &[String],
    attempted: &[usize],
    conversation_id: Option<&str>,
) -> Option<usize> {
    let conversation_id = conversation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if tokens.len() < 2 {
        return None;
    }
    let attempted_set: HashSet<usize> = attempted.iter().copied().collect();
    stable_candidate_order(
        tokens,
        |index, token| sticky_token_score(conversation_id, server_id, index, token),
        |left_index, _, right_index, _| left_index.cmp(&right_index),
    )
    .into_iter()
    .find(|index| !attempted_set.contains(index))
}

fn sticky_token_score(
    conversation_id: &str,
    server_id: uuid::Uuid,
    token_index: usize,
    token: &str,
) -> [u8; 32] {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;

    hasher.update(conversation_id.as_bytes());
    hasher.update(server_id.as_bytes());
    hasher.update(token_index.to_le_bytes());
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn least_used_selection_balances_and_breaks_ties_by_order() {
        let balancer = McpBearerTokenBalancer::new();
        let server_id = uuid::Uuid::new_v4();
        let tokens = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let picks = [
            balancer.select_token(server_id, &tokens, &[], None).await,
            balancer.select_token(server_id, &tokens, &[], None).await,
            balancer.select_token(server_id, &tokens, &[], None).await,
            balancer.select_token(server_id, &tokens, &[], None).await,
        ];

        assert_eq!(picks[0].value.as_deref(), Some("a"));
        assert_eq!(picks[1].value.as_deref(), Some("b"));
        assert_eq!(picks[2].value.as_deref(), Some("c"));
        assert_eq!(picks[3].value.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn selection_skips_attempted_indices() {
        let balancer = McpBearerTokenBalancer::new();
        let server_id = uuid::Uuid::new_v4();
        let tokens = vec!["a".to_string(), "b".to_string()];

        let first = balancer.select_token(server_id, &tokens, &[], None).await;
        let second = balancer
            .select_token(server_id, &tokens, &[first.index.expect("index")], None)
            .await;

        assert_eq!(first.value.as_deref(), Some("a"));
        assert_eq!(second.value.as_deref(), Some("b"));
    }

    #[tokio::test]
    async fn sticky_selection_stays_stable_for_same_conversation() {
        let balancer = McpBearerTokenBalancer::new();
        let server_id = uuid::Uuid::new_v4();
        let tokens = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let first = balancer
            .select_token(server_id, &tokens, &[], Some("conv-a"))
            .await;
        let second = balancer
            .select_token(server_id, &tokens, &[], Some("conv-a"))
            .await;

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn sticky_retry_uses_next_ranked_token() {
        let balancer = McpBearerTokenBalancer::new();
        let server_id = uuid::Uuid::new_v4();
        let tokens = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let first = balancer
            .select_token(server_id, &tokens, &[], Some("conv-a"))
            .await;
        let second = balancer
            .select_token(
                server_id,
                &tokens,
                &[first.index.expect("index")],
                Some("conv-a"),
            )
            .await;

        assert_ne!(first.index, second.index);
    }

    #[test]
    fn sticky_selection_spreads_across_conversations() {
        let server_id = uuid::Uuid::new_v4();
        let tokens = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        let mut seen = std::collections::HashSet::new();
        for index in 0..64 {
            let conversation_id = format!("conv-{index}");
            seen.insert(sticky_token_index(
                server_id,
                &tokens,
                &[],
                Some(conversation_id.as_str()),
            ));
        }

        assert!(seen.len() >= 2);
    }
}
