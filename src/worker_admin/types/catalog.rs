//! The safe, read-only catalog contracts.
//!
//! These shapes are the only thing an ordinary authenticated user ever sees
//! of the upstream endpoints, the model routes, and the cached quota. Every
//! field is an explicit allowlist entry: no base URL, no owner or scope
//! identifier, no credential, key label, proxy setting, secret-presence
//! flag, target id, provider error, or spend metadata reaches them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    config::NativeApi,
    db::{
        EndpointPlan, EndpointProvider, EndpointRegion, ModelRouteRoutingStrategy,
        UnifiedModelRoute, UnifiedProviderEndpoint,
    },
    worker_admin_types::{CanonicalQuotaWindow, TokenPlanKeyUsage, TokenPlanUsageResponse},
};

/// How a visible catalog row is scoped for the current viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogVisibility {
    /// An administrator-owned resource every authenticated user may read.
    Shared,
    /// A user-scoped resource the viewer owns.
    Owned,
}

impl CatalogVisibility {
    /// The label the safe catalog answers with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Owned => "owned",
        }
    }
}

impl From<crate::db::visibility::CatalogVisibility> for CatalogVisibility {
    fn from(value: crate::db::visibility::CatalogVisibility) -> Self {
        match value {
            crate::db::visibility::CatalogVisibility::Shared => Self::Shared,
            crate::db::visibility::CatalogVisibility::Owned => Self::Owned,
        }
    }
}

/// One upstream endpoint of the safe catalog.
///
/// The row never names the endpoint address, the owner, the credential, or
/// the key labels; it only describes what the viewer may route through.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct EndpointCatalogRow {
    pub endpoint_id: Uuid,
    pub name: String,
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    pub plan: EndpointPlan,
    pub enabled: bool,
    pub native_api: NativeApi,
    pub native_api_source: String,
    pub visibility: CatalogVisibility,
    /// `true` when the provider exposes a quota surface for this endpoint.
    pub quota_eligible: bool,
}

impl EndpointCatalogRow {
    /// Project one stored endpoint into the safe row.
    ///
    /// The projection copies the allowlisted fields only: the address, the
    /// credential rows, the proxy state, the owner, and the scope identifier
    /// never leave this function.
    pub fn from_endpoint(
        endpoint: &UnifiedProviderEndpoint,
        visibility: CatalogVisibility,
    ) -> Self {
        Self {
            endpoint_id: endpoint.endpoint_id,
            name: endpoint.name.clone(),
            provider: endpoint.provider,
            provider_region: endpoint.provider_region,
            plan: endpoint.plan,
            enabled: endpoint.enabled,
            native_api: endpoint.native_api,
            native_api_source: endpoint.native_api_source.clone(),
            visibility,
            quota_eligible: quota_eligible(endpoint.provider, endpoint.plan),
        }
    }
}

/// One page of the safe endpoint catalog.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct EndpointCatalogPageResponse {
    pub total: i64,
    pub endpoints: Vec<EndpointCatalogRow>,
    pub first: i64,
    pub rows: i64,
}

/// One target of a safe model-route row.
///
/// A target never names the upstream address, the credential, the proxy, or
/// the internal target identity.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelRouteCatalogTarget {
    pub endpoint_id: Uuid,
    pub endpoint_name: Option<String>,
    pub endpoint_enabled: bool,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub native_api: NativeApi,
}

/// One model route of the safe catalog.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelRouteCatalogRow {
    pub rule_id: Uuid,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub enabled: bool,
    pub visibility: CatalogVisibility,
    /// The targets the viewer may see. A route without a visible target keeps
    /// an empty list instead of naming or counting a hidden target.
    pub targets: Vec<ModelRouteCatalogTarget>,
}

impl ModelRouteCatalogRow {
    /// Project one stored rule into the safe row.
    ///
    /// Only the targets whose endpoint the viewer may see survive the
    /// projection; a hidden target is dropped instead of being named.
    pub fn from_route(
        route: &UnifiedModelRoute,
        visibility: CatalogVisibility,
        visible_endpoint_ids: &HashSet<Uuid>,
    ) -> Self {
        let targets = route
            .targets
            .iter()
            .filter(|target| visible_endpoint_ids.contains(&target.endpoint_id))
            .map(|target| ModelRouteCatalogTarget {
                endpoint_id: target.endpoint_id,
                endpoint_name: target.endpoint_name.clone(),
                endpoint_enabled: target.endpoint_enabled,
                enabled: target.enabled,
                upstream_model: target.upstream_model.clone(),
                native_api: target.native_api,
            })
            .collect();
        Self {
            rule_id: route.rule_id,
            model_pattern: route.model_pattern.clone(),
            routing_strategy: route.routing_strategy,
            enabled: route.enabled,
            visibility,
            targets,
        }
    }
}

/// One page of the safe model-route catalog.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelRouteCatalogPageResponse {
    pub total: i64,
    pub routes: Vec<ModelRouteCatalogRow>,
    pub first: i64,
    pub rows: i64,
}

/// One cached quota observation of one cached key.
///
/// The observation carries the canonical quota windows only: no key label,
/// no key or account id, no provider error, and no spend metadata.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct CachedQuotaObservation {
    /// The moment the cached value was observed, when the cache knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    /// `true` when the cached value is past its freshness window.
    pub stale: bool,
    pub windows: Vec<CanonicalQuotaWindow>,
}

/// The cached quota of one visible endpoint.
///
/// The answer is always served from the cache: a read never collects quota,
/// never refreshes it, and never reaches the upstream provider.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct CachedQuotaResponse {
    pub endpoint_id: Uuid,
    pub provider: EndpointProvider,
    pub provider_region: Option<EndpointRegion>,
    /// `false` when no usable cached observation exists. An unknown balance is
    /// never reported as zero.
    pub available: bool,
    pub observations: Vec<CachedQuotaObservation>,
}

impl CachedQuotaResponse {
    /// Build the safe answer from the projected observations.
    ///
    /// `available` follows the canonical window rows, never the existence of
    /// a cache entry or of a key: an observation without a usable window is
    /// not an observation, and an answer without a usable window is
    /// unavailable.
    pub(crate) fn new(
        endpoint_id: Uuid,
        provider: EndpointProvider,
        provider_region: Option<EndpointRegion>,
        observations: Vec<CachedQuotaObservation>,
    ) -> Self {
        let available = observations
            .iter()
            .any(|observation| !observation.windows.is_empty());
        Self {
            endpoint_id,
            provider,
            provider_region,
            available,
            observations,
        }
    }
}

/// The safe observation of one cached key.
///
/// A key without a canonical window row carries no usable quota, so it never
/// becomes an observation: an unusable snapshot stays unavailable instead of
/// reporting an empty quota as available.
pub(crate) fn cached_observation(
    key: &TokenPlanKeyUsage,
    observed_at: Option<DateTime<Utc>>,
    stale: bool,
) -> Option<CachedQuotaObservation> {
    let windows = key.quota_windows.clone().unwrap_or_default();
    if windows.is_empty() {
        return None;
    }
    Some(CachedQuotaObservation {
        observed_at,
        stale,
        windows,
    })
}

/// The safe observations of one persisted provider response.
///
/// Every successful key that carries at least one canonical window becomes
/// one observation. The provider error, the spend metadata, and the key
/// identity stay in the response envelope and never reach the safe answer.
pub(crate) fn cached_observations_from_usage(
    usage: &TokenPlanUsageResponse,
) -> Vec<CachedQuotaObservation> {
    let mut usage = usage.clone();
    crate::worker_admin::quota_canonical::populate_response_observations(
        usage.provider,
        &mut usage,
    );
    usage
        .keys
        .iter()
        .filter(|key| key.ok)
        .filter_map(|key| {
            let observation = key
                .model_remains
                .iter()
                .find_map(|model| model.observation.as_ref());
            cached_observation(
                key,
                observation.and_then(|observation| observation.observed_at),
                observation.is_some_and(|observation| observation.stale),
            )
        })
        .collect()
}

/// Whether the provider of an endpoint exposes a quota surface.
///
/// A generic endpoint has no token-plan or subscription quota surface; an
/// OpenAI endpoint exposes one only with the ChatGPT subscription plan.
pub fn quota_eligible(provider: EndpointProvider, plan: EndpointPlan) -> bool {
    match provider {
        EndpointProvider::Generic => false,
        EndpointProvider::OpenAi => plan == EndpointPlan::ChatgptSubscription,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{UnifiedEndpointApiKey, UnifiedModelRouteTarget};
    use crate::worker_admin_types::{
        SubscriptionQuotaObservation, SubscriptionWindowAvailability, SubscriptionWindowUsage,
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse,
    };
    use serde_json::Value;

    const SECRET_KEY: &str = "SENTINEL-API-KEY";
    const SECRET_URL: &str = "https://user:SENTINEL-PASSWORD@upstream.example/v1";
    const SECRET_LABEL: &str = "SENTINEL-KEY-LABEL";
    const SECRET_ERROR: &str = "SENTINEL-PROVIDER-ERROR";
    const SECRET_ACCOUNT: &str = "SENTINEL-ACCOUNT-ID";

    fn endpoint_fixture(scope: &str, owner_user_id: Option<i64>) -> UnifiedProviderEndpoint {
        UnifiedProviderEndpoint {
            endpoint_id: Uuid::nil(),
            scope: scope.to_string(),
            owner_user_id,
            name: "primary".to_string(),
            provider: EndpointProvider::Minimax,
            provider_region: Some(EndpointRegion::Cn),
            plan: EndpointPlan::PlatformApiKey,
            service_tier: Some("fast".to_string()),
            base_url: SECRET_URL.to_string(),
            native_api: NativeApi::Chat,
            native_api_source: "manual".to_string(),
            key_lb_enabled: true,
            enabled: true,
            mcp_enabled: false,
            has_proxy_url: true,
            has_oauth_token: true,
            has_admin_api_key: true,
            active_windows: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            api_keys: vec![UnifiedEndpointApiKey {
                key_id: Uuid::nil(),
                endpoint_id: Uuid::nil(),
                key_label: SECRET_LABEL.to_string(),
                position: 0,
                enabled: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
        }
    }

    fn target(endpoint_id: Uuid) -> UnifiedModelRouteTarget {
        UnifiedModelRouteTarget {
            target_id: Uuid::nil(),
            rule_id: Uuid::nil(),
            endpoint_id,
            endpoint_name: Some("primary".to_string()),
            endpoint_enabled: true,
            position: 0,
            enabled: true,
            upstream_model: Some("gpt-4o".to_string()),
            native_api: NativeApi::Chat,
            has_proxy_url_override: true,
            active_windows: Vec::new(),
            dev_system_normalize: true,
            thinking_downgrade_enabled: true,
            thinking_effort_override: Some("high".to_string()),
            compact_mode: crate::db::CompactMode::Passthrough,
            service_tier: Some("fast".to_string()),
            embedding_dimensions_override: Some(1024),
            prefer_new_sessions: true,
        }
    }

    fn route_fixture() -> UnifiedModelRoute {
        UnifiedModelRoute {
            rule_id: Uuid::nil(),
            scope: "user".to_string(),
            owner_user_id: Some(7),
            model_pattern: "gpt-*".to_string(),
            routing_strategy: ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            enabled: true,
            targets: vec![target(Uuid::nil())],
        }
    }

    /// Every field name the safe contracts may ever carry.
    fn allowed_field_names() -> Vec<&'static str> {
        vec![
            "endpoint_id",
            "name",
            "provider",
            "provider_region",
            "plan",
            "enabled",
            "native_api",
            "native_api_source",
            "visibility",
            "quota_eligible",
            "total",
            "endpoints",
            "first",
            "rows",
            "rule_id",
            "model_pattern",
            "routing_strategy",
            "targets",
            "endpoint_name",
            "endpoint_enabled",
            "upstream_model",
            "observed_at",
            "stale",
            "windows",
            "available",
            "observations",
            "source_id",
            "kind",
            "source_model",
            "unit",
            "duration_seconds",
            "is_default_scope",
            "availability",
            "remaining_percent",
            "current",
            "limit",
            "remaining",
            "reset_at",
        ]
    }

    /// Walk a serialized answer and collect every field name it carries.
    fn field_names(value: &Value, names: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    names.push(key.clone());
                    field_names(child, names);
                }
            }
            Value::Array(items) => {
                for item in items {
                    field_names(item, names);
                }
            }
            _ => {}
        }
    }

    /// Assert the serialized answer carries no forbidden field name and no
    /// sentinel secret value.
    fn assert_safe(value: &Value) {
        let mut names = Vec::new();
        field_names(value, &mut names);
        for name in &names {
            assert!(
                allowed_field_names().contains(&name.as_str()),
                "unexpected field {name} in the safe answer"
            );
        }
        let rendered = value.to_string();
        for secret in [
            SECRET_KEY,
            SECRET_URL,
            SECRET_LABEL,
            SECRET_ERROR,
            SECRET_ACCOUNT,
        ] {
            assert!(
                !rendered.contains(secret),
                "the safe answer leaked {secret}"
            );
        }
    }

    #[test]
    fn the_endpoint_catalog_row_drops_every_secret_field() {
        let endpoint = endpoint_fixture("user", Some(7));
        let row = EndpointCatalogRow::from_endpoint(&endpoint, CatalogVisibility::Owned);
        let value = serde_json::to_value(row).expect("the row serializes");
        assert_safe(&value);
        assert_eq!(value["visibility"], "owned");
    }

    #[test]
    fn the_model_route_catalog_row_drops_every_secret_field() {
        let route = route_fixture();
        let visible = HashSet::from([Uuid::nil()]);
        let row = ModelRouteCatalogRow::from_route(&route, CatalogVisibility::Owned, &visible);
        let value = serde_json::to_value(row).expect("the row serializes");
        assert_safe(&value);
    }

    #[test]
    fn a_hidden_target_never_reaches_the_model_route_catalog_row() {
        let route = route_fixture();
        let hidden = Uuid::from_u128(9);
        let visible = HashSet::from([Uuid::nil()]);
        let row = ModelRouteCatalogRow::from_route(&route, CatalogVisibility::Owned, &visible);
        assert_eq!(row.targets.len(), 1);
        assert_eq!(row.targets[0].endpoint_id, Uuid::nil());

        // A route whose only target is hidden keeps an empty target list; it
        // never names or counts the hidden target.
        let mut hidden_only = route_fixture();
        hidden_only.targets = vec![target(hidden)];
        let row =
            ModelRouteCatalogRow::from_route(&hidden_only, CatalogVisibility::Owned, &visible);
        assert!(row.targets.is_empty());
        let value = serde_json::to_value(row).expect("the row serializes");
        assert_safe(&value);
        assert!(!value.to_string().contains(&hidden.to_string()));
    }

    #[test]
    fn the_cached_quota_answer_carries_only_allowed_fields() {
        let response = CachedQuotaResponse {
            endpoint_id: Uuid::nil(),
            provider: EndpointProvider::DeepSeek,
            provider_region: None,
            available: true,
            observations: vec![CachedQuotaObservation {
                observed_at: None,
                stale: false,
                windows: Vec::new(),
            }],
        };
        let value = serde_json::to_value(response).expect("the response serializes");
        assert_safe(&value);
    }

    #[test]
    fn the_quota_eligibility_follows_the_provider_and_the_plan() {
        assert!(!quota_eligible(
            EndpointProvider::Generic,
            EndpointPlan::PlatformApiKey
        ));
        assert!(!quota_eligible(
            EndpointProvider::OpenAi,
            EndpointPlan::PlatformApiKey
        ));
        assert!(quota_eligible(
            EndpointProvider::OpenAi,
            EndpointPlan::ChatgptSubscription
        ));
        assert!(quota_eligible(
            EndpointProvider::Minimax,
            EndpointPlan::PlatformApiKey
        ));
    }

    /// A persisted provider response with one successful key that carries no
    /// canonical window row.
    fn persisted_usage_without_windows() -> TokenPlanUsageResponse {
        TokenPlanUsageResponse {
            provider: EndpointProvider::OpenAi,
            provider_region: None,
            local_today_tokens: None,
            quota_inference_available: None,
            keys: vec![persisted_key(Uuid::nil(), Vec::new())],
        }
    }

    /// One persisted successful key with the given canonical window slots.
    fn persisted_key(key_id: Uuid, windows: Vec<SubscriptionWindowUsage>) -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id,
            key_label: "key".to_string(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            quota_status: None,
            model_remains: vec![TokenPlanModelUsage {
                model_name: "plus".to_string(),
                interval: None,
                weekly: None,
                windows: Some(windows),
                observation: Some(SubscriptionQuotaObservation {
                    observed_at: Some(chrono::Utc::now()),
                    source: Some("snapshot".to_string()),
                    stale: false,
                    last_error_code: None,
                    next_retry_at: None,
                    refreshing: false,
                }),
            }],
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
            quota_windows: None,
        }
    }

    fn persisted_window() -> SubscriptionWindowUsage {
        SubscriptionWindowUsage {
            source_window: "primary".to_string(),
            window_seconds: Some(18_000),
            used_percent: Some(25.0),
            remaining_percent: Some(75.0),
            reset_at: Some(chrono::Utc::now() + chrono::Duration::seconds(600)),
            reset_after_seconds: Some(600),
            availability: SubscriptionWindowAvailability::Known,
        }
    }

    /// A successful persisted snapshot without a usable window is not an
    /// available quota.
    #[test]
    fn a_persisted_snapshot_without_windows_is_unavailable() {
        let usage = persisted_usage_without_windows();
        let observations = cached_observations_from_usage(&usage);
        assert!(
            observations.is_empty(),
            "a successful key without a canonical window carries no quota"
        );
        let response =
            CachedQuotaResponse::new(Uuid::nil(), EndpointProvider::OpenAi, None, observations);
        assert!(
            !response.available,
            "an empty snapshot must not report an available quota"
        );
        assert!(response.observations.is_empty());
    }

    /// A mixed persisted snapshot keeps every usable observation and drops
    /// the unusable one.
    #[test]
    fn a_mixed_persisted_snapshot_keeps_only_the_usable_observations() {
        let usable_key = Uuid::from_u128(1);
        let mut usage = persisted_usage_without_windows();
        usage
            .keys
            .push(persisted_key(usable_key, vec![persisted_window()]));
        let observations = cached_observations_from_usage(&usage);
        assert_eq!(
            observations.len(),
            1,
            "the key without a canonical window never becomes an observation"
        );
        let observation = &observations[0];
        assert!(observation.observed_at.is_some());
        assert!(!observation.stale);
        assert_eq!(observation.windows.len(), 1);
        let window = &observation.windows[0];
        assert_eq!(window.source_id, "subscription:primary");
        assert_eq!(window.remaining_percent, Some(75.0));
        assert_eq!(window.duration_seconds, Some(18_000));
        assert!(window.reset_at.is_some());
        let response =
            CachedQuotaResponse::new(Uuid::nil(), EndpointProvider::OpenAi, None, observations);
        assert!(response.available);
        assert_eq!(response.observations.len(), 1);
    }
}
