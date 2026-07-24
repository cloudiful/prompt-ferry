use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::config::NativeApi;
use crate::db::RouteSelectionReason;
use crate::db::types::EndpointApiKey;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteRoutingStrategy {
    #[default]
    ClientKeyRendezvous,
    ResponsesSessionAffinity,
}

impl ModelRouteRoutingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientKeyRendezvous => "client_key_rendezvous",
            Self::ResponsesSessionAffinity => "responses_session_affinity",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesContinuationPolicy {
    ForcePassthrough,
    ForceReplay,
}

impl ResponsesContinuationPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ForcePassthrough => "force_passthrough",
            Self::ForceReplay => "force_replay",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct StreamDeltaBatchingSettings {
    pub enabled: bool,
    pub flush_window_ms: u64,
    pub max_buffer_chars: usize,
    pub max_buffer_bytes: usize,
    pub flush_on_line_break: bool,
    pub flush_on_sentence_end: bool,
}

impl Default for StreamDeltaBatchingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            flush_window_ms: 50,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct RouteConfig {
    pub route_id: uuid::Uuid,
    pub user_id: i64,
    pub model_route_rule_id: Option<uuid::Uuid>,
    pub base_url: String,
    pub api_key: String,
    pub endpoint_key_id: Option<uuid::Uuid>,
    pub endpoint_key_label: Option<String>,
    pub api_keys: Vec<EndpointApiKey>,
    pub key_lb_enabled: bool,
    pub native_api: NativeApi,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: ResponsesContinuationPolicy,
    pub route_selection_reason: RouteSelectionReason,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ModelRouteTarget {
    pub target_id: uuid::Uuid,
    pub rule_id: uuid::Uuid,
    pub endpoint_id: uuid::Uuid,
    pub endpoint_name: Option<String>,
    pub endpoint_enabled: bool,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: ResponsesContinuationPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelEndpointRule {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub session_affinity_lock_after_turns: i32,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub targets: Vec<ModelRouteTarget>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ModelEndpointRuleRow {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: String,
    pub session_affinity_lock_after_turns: i32,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ModelRouteTargetCreate {
    pub endpoint_id: uuid::Uuid,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: ResponsesContinuationPolicy,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ModelEndpointRuleCreate {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub session_affinity_lock_after_turns: i32,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub targets: Vec<ModelRouteTargetCreate>,
}

#[derive(Debug, Clone)]
pub struct ModelRouteCandidate {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub session_affinity_lock_after_turns: i32,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub updated_at: DateTime<Utc>,
    pub targets: Vec<ModelRouteCandidateTarget>,
}

#[derive(Debug, Clone)]
pub struct ModelRouteCandidateTarget {
    pub target_id: uuid::Uuid,
    pub endpoint_id: uuid::Uuid,
    pub endpoint_name: String,
    pub base_url: String,
    pub api_key: String,
    pub api_keys: Vec<EndpointApiKey>,
    pub key_lb_enabled: bool,
    pub native_api: NativeApi,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: ResponsesContinuationPolicy,
}

#[derive(Debug, Clone)]
pub struct RouteTestEndpoint {
    pub endpoint_id: uuid::Uuid,
    pub name: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct SnapshotKey {
    pub key_hash: String,
    pub key_prefix: String,
    pub user_id: i64,
    pub route_id: uuid::Uuid,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelRoutePage {
    pub total: i64,
    pub routes: Vec<ModelEndpointRule>,
    pub first: i64,
    pub rows: i64,
}
