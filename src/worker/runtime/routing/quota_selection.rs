use crate::{
    db,
    worker::runtime::{context::RuntimeServices, request_assembly::BufferedBridgeRequest},
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
};
use sha2::{Digest, Sha256};
use tracing::warn;

pub(super) fn select_quota_key<'a>(
    cache: &TokenPlanQuotaCache,
    endpoint_id: uuid::Uuid,
    available_keys: &'a [&'a db::EndpointApiKey],
    model: Option<&str>,
    stable_key: String,
    estimated_tokens: u64,
) -> Option<&'a db::EndpointApiKey> {
    let weighted = available_keys
        .iter()
        .filter_map(|key| {
            cache
                .key_remaining_percent_now(endpoint_id, key.key_id, model)
                .filter(|remaining| *remaining > 0.0)
                .map(|remaining| (*key, remaining))
        })
        .collect::<Vec<_>>();
    if weighted.is_empty() {
        return None;
    }

    let total = weighted
        .iter()
        .map(|(_, remaining)| *remaining)
        .sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"minimax-quota");
    hasher.update(stable_key.as_bytes());
    hasher.update(endpoint_id.as_bytes());
    let digest = hasher.finalize();
    let bucket = u64::from_be_bytes(digest[..8].try_into().expect("sha256 has eight bytes"));
    let point = (bucket as f64 / u64::MAX as f64) * total;
    let fallback_key = weighted.last().map(|(key, _)| *key);
    let mut cumulative = 0.0;
    for (key, remaining) in weighted {
        cumulative += remaining;
        if point < cumulative {
            cache.reserve_estimated_tokens(endpoint_id, key.key_id, estimated_tokens);
            return Some(key);
        }
    }
    if let Some(key) = fallback_key {
        cache.reserve_estimated_tokens(endpoint_id, key.key_id, estimated_tokens);
    }
    fallback_key
}

pub(super) fn request_model(request: &BufferedBridgeRequest) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .ok()?
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

pub(super) fn stable_endpoint_api_key_score(
    stable_key: &str,
    key: &db::EndpointApiKey,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(stable_key.as_bytes());
    hasher.update(key.endpoint_id.as_bytes());
    hasher.update(key.key_id.as_bytes());
    if key.key_id.is_nil() {
        hasher.update(key.key_label.as_bytes());
    }
    hasher.finalize().into()
}

pub(super) async fn refresh_quota_if_due(services: &RuntimeServices, endpoint_id: uuid::Uuid) {
    let Some(state) = services.admin_state() else {
        return;
    };
    if let Err(error) = state
        .token_plan_quota
        .refresh_if_due(&state.pool, endpoint_id)
        .await
    {
        warn!(
            endpoint_id = %endpoint_id,
            error = %error,
            "MiniMax quota refresh failed; retaining the previous quota snapshot"
        );
    }
}
