use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    db,
    worker_admin_state::{AdminState, error, internal},
};
use axum::{http::StatusCode, response::Response};

use super::validate_request_budget_limit;

#[derive(Debug, Deserialize, ToSchema)]
pub struct ModelRouteRequest {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: Option<crate::db::ModelRouteRoutingStrategy>,
    pub session_affinity_lock_after_turns: Option<i32>,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: Option<bool>,
    pub endpoint_id: Option<Uuid>,
    pub priority: Option<i32>,
    pub targets: Option<Vec<ModelRouteTargetRequest>>,
}

impl ModelRouteRequest {
    pub async fn validate_for_create(&self, state: &AdminState) -> Result<(), Response> {
        self.validate(state, None).await
    }

    pub async fn validate_for_update(
        &self,
        state: &AdminState,
        existing_rule_id: Uuid,
    ) -> Result<(), Response> {
        self.validate(state, Some(existing_rule_id)).await
    }

    pub async fn into_create(
        self,
        state: &AdminState,
    ) -> Result<db::ModelEndpointRuleCreate, Response> {
        let targets = self
            .targets
            .unwrap_or_else(|| {
                self.endpoint_id
                    .iter()
                    .copied()
                    .map(|endpoint_id| ModelRouteTargetRequest {
                        endpoint_id,
                        enabled: Some(true),
                        upstream_model: None,
                        responses_continuation_policy: None,
                    })
                    .collect()
            })
            .into_iter()
            .map(|target| async move {
                let endpoint = db::get_endpoint(&state.pool, target.endpoint_id)
                    .await
                    .map_err(|err| internal(state, err))?
                    .ok_or_else(|| {
                        error(
                            StatusCode::BAD_REQUEST,
                            "invalid_target_endpoint",
                            "target endpoint not found",
                        )
                    })?;
                let responses_continuation_policy = target.responses_continuation_policy.unwrap_or(
                    if endpoint.native_api == "responses" {
                        db::ResponsesContinuationPolicy::ForcePassthrough
                    } else {
                        db::ResponsesContinuationPolicy::ForceReplay
                    },
                );
                if matches!(
                    responses_continuation_policy,
                    db::ResponsesContinuationPolicy::ForcePassthrough
                ) && endpoint.native_api != "responses"
                {
                    return Err(error(
                        StatusCode::BAD_REQUEST,
                        "invalid_target_continuation_policy",
                        "force_passthrough requires a responses-native endpoint target",
                    ));
                }
                Ok(db::ModelRouteTargetCreate {
                    endpoint_id: target.endpoint_id,
                    enabled: target.enabled.unwrap_or(true),
                    upstream_model: target
                        .upstream_model
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    responses_continuation_policy,
                })
            });
        let targets = futures::future::try_join_all(targets).await?;
        Ok(db::ModelEndpointRuleCreate {
            scope: self.scope,
            owner_user_id: self.owner_user_id,
            model_pattern: self.model_pattern,
            routing_strategy: self.routing_strategy.unwrap_or_default(),
            session_affinity_lock_after_turns: self.session_affinity_lock_after_turns.unwrap_or(5),
            daily_max_requests: self.daily_max_requests,
            monthly_max_requests: self.monthly_max_requests,
            enabled: self.enabled.unwrap_or(true),
            targets,
        })
    }

    async fn validate(
        &self,
        state: &AdminState,
        existing_rule_id: Option<Uuid>,
    ) -> Result<(), Response> {
        validate_request_budget_limit(self.daily_max_requests, "daily_max_requests")
            .map_err(|response| *response)?;
        validate_request_budget_limit(self.monthly_max_requests, "monthly_max_requests")
            .map_err(|response| *response)?;
        if self
            .session_affinity_lock_after_turns
            .is_some_and(|turns| turns <= 0)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_session_affinity_lock_after_turns",
                "session_affinity_lock_after_turns must be greater than zero",
            ));
        }
        let pattern = self.model_pattern.trim();
        if pattern.is_empty() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_model_pattern",
                "model pattern is required",
            ));
        }
        if !matches!(self.scope.as_str(), "admin" | "user") {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                "scope must be admin or user",
            ));
        }
        if self.scope == "admin" && self.owner_user_id.is_some() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "admin route cannot have owner",
            ));
        }
        if self.scope == "user" && self.owner_user_id.is_none() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "user route requires owner",
            ));
        }
        let routing_strategy = self.routing_strategy.unwrap_or_default();
        if routing_strategy == db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
            && self
                .targets
                .as_ref()
                .map_or(self.endpoint_id.is_none(), |targets| targets.len() < 2)
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_routing_strategy",
                "responses_session_affinity requires at least two route targets",
            ));
        }
        let targets = self.targets.as_deref().unwrap_or(&[]);
        let has_legacy_endpoint = self.endpoint_id.is_some();
        if targets.is_empty() && !has_legacy_endpoint {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_targets",
                "model route requires at least one target",
            ));
        }
        let target_ids = if targets.is_empty() {
            self.endpoint_id.iter().copied().collect::<Vec<_>>()
        } else {
            targets
                .iter()
                .map(|target| target.endpoint_id)
                .collect::<Vec<_>>()
        };
        let unique_target_ids = target_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        if unique_target_ids.len() != target_ids.len() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "duplicate_targets",
                "model route target endpoints must be unique",
            ));
        }
        for endpoint_id in unique_target_ids {
            let endpoint = db::get_endpoint(&state.pool, endpoint_id)
                .await
                .map_err(|err| internal(state, err))?;
            if endpoint.is_none() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_target_endpoint",
                    "target endpoint not found",
                ));
            }
        }
        let rules = db::list_model_endpoint_rules(&state.pool)
            .await
            .map_err(|err| internal(state, err))?;
        if rules.iter().any(|rule| {
            Some(rule.rule_id) != existing_rule_id
                && rule.scope == self.scope
                && rule.owner_user_id == self.owner_user_id
                && rule.model_pattern == pattern
        }) {
            return Err(error(
                StatusCode::CONFLICT,
                "duplicate_model_route",
                "model route pattern already exists for scope/owner",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ModelRouteTargetRequest {
    pub endpoint_id: Uuid,
    pub enabled: Option<bool>,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: Option<db::ResponsesContinuationPolicy>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ModelRouteTestRequest {
    pub rule_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModelRouteTestResponse {
    pub ok: bool,
    pub status: Option<u16>,
    #[schema(value_type = u64)]
    pub duration_ms: u128,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub preferred_endpoint_id: Option<Uuid>,
    pub preferred_endpoint_name: Option<String>,
    pub rule_id: Option<Uuid>,
    pub model_pattern: Option<String>,
    pub model: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelRouteWhitelistResponse {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ModelRouteWhitelistRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ModelRoutePageResponse {
    pub total: i64,
    pub routes: Vec<db::ModelEndpointRule>,
    pub first: i64,
    pub rows: i64,
}

impl From<db::ModelRoutePage> for ModelRoutePageResponse {
    fn from(value: db::ModelRoutePage) -> Self {
        Self {
            total: value.total,
            routes: value.routes,
            first: value.first,
            rows: value.rows,
        }
    }
}
