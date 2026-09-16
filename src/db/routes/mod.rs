use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

use crate::config::NativeApi;
use crate::db::types::{
    EndpointApiKey, ModelEndpointRule, ModelEndpointRuleCreate, ModelEndpointRuleRow,
    ModelRouteCandidate, ModelRouteCandidateTarget, ModelRoutePage, ModelRouteRoutingStrategy,
    ModelRouteTarget, RouteConfig, SnapshotKey,
};

mod active_windows;
mod hydrate;
mod matching;
mod mutations;
mod queries;
mod snapshot;

pub use crate::db::types::ActiveWindow;
pub use active_windows::{
    candidate_target_is_active, effective_stored_is_active_at, effective_windows_raw,
    format_minutes, is_active_at, normalize_request_windows, parse_stored_windows,
    schedule_unavailable_message, storage_value, stored_is_active_at, summarize_effective_stored,
    summarize_stored, summarize_windows, worker_local_hhmm_now, worker_local_minutes_now,
};

pub use matching::{cleanup_orphan_model_routes, get_route, model_pattern_matches};
pub use mutations::{
    create_model_endpoint_rule, delete_model_endpoint_rule, update_model_endpoint_rule,
};
pub use queries::{
    get_model_endpoint_rule, get_model_route_candidate, list_model_endpoint_rules,
    list_model_endpoint_rules_page, list_visible_model_route_endpoints,
    list_visible_model_route_endpoints_strict, model_route_candidates,
};
pub use snapshot::{
    effective_route, resolve_model_route, resolve_model_route_with_fallback, snapshot_keys,
};

#[derive(Debug, Clone, sqlx::FromRow)]
struct ModelRouteCandidateRow {
    rule_id: uuid::Uuid,
    scope: String,
    owner_user_id: Option<i64>,
    model_pattern: String,
    routing_strategy: String,
    updated_at: DateTime<Utc>,
    target_id: uuid::Uuid,
    endpoint_id: uuid::Uuid,
    endpoint_name: String,
    base_url: String,
    api_key: String,
    proxy_url: Option<String>,
    key_lb_enabled: bool,
    native_api: String,
    target_native_api: Option<String>,
    provider: String,
    service_tier: Option<String>,
    position: i32,
    target_enabled: bool,
    upstream_model: Option<String>,
    proxy_url_override: Option<String>,
    active_windows: Option<String>,
    endpoint_active_windows: Option<String>,
    dev_system_normalize: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ModelRouteTargetRow {
    target_id: uuid::Uuid,
    rule_id: uuid::Uuid,
    endpoint_id: uuid::Uuid,
    endpoint_name: Option<String>,
    endpoint_enabled: bool,
    position: i32,
    enabled: bool,
    upstream_model: Option<String>,
    native_api: Option<String>,
    proxy_url_override: Option<String>,
    active_windows: Option<String>,
    dev_system_normalize: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ClientKeyRow {
    key_hash: String,
    key_prefix: String,
    user_id: i64,
}

fn fallback_api_keys(
    endpoint_id: uuid::Uuid,
    endpoint_name: &str,
    api_key: &str,
) -> Vec<EndpointApiKey> {
    if api_key.trim().is_empty() {
        return Vec::new();
    }
    vec![EndpointApiKey {
        key_id: uuid::Uuid::nil(),
        endpoint_id,
        key_label: endpoint_name.to_string(),
        api_key: api_key.to_string(),
        position: 0,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }]
}

fn parse_native_api(value: &str) -> NativeApi {
    serde_json::from_value(serde_json::Value::String(value.to_string())).unwrap_or(NativeApi::Auto)
}

fn parse_routing_strategy(value: &str) -> ModelRouteRoutingStrategy {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .unwrap_or(ModelRouteRoutingStrategy::ClientKeyRendezvous)
}
