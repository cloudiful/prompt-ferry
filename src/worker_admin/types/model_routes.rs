use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    db,
    worker_admin_state::{AdminState, ApiError},
};
use axum::http::StatusCode;

#[derive(Debug, Deserialize, ToSchema)]
pub struct ModelRouteRequest {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: Option<crate::db::ModelRouteRoutingStrategy>,
    pub enabled: Option<bool>,
    pub endpoint_id: Option<Uuid>,
    pub priority: Option<i32>,
    pub targets: Option<Vec<ModelRouteTargetRequest>>,
}

impl ModelRouteRequest {
    pub async fn validate_for_create(&self, state: &AdminState) -> Result<(), ApiError> {
        self.validate(state, None).await
    }

    pub async fn validate_for_update(
        &self,
        state: &AdminState,
        existing_rule_id: Uuid,
    ) -> Result<(), ApiError> {
        self.validate(state, Some(existing_rule_id)).await
    }

    pub async fn into_create(
        self,
        state: &AdminState,
    ) -> Result<db::ModelEndpointRuleCreate, ApiError> {
        self.into_create_with_existing(
            state,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &DimensionOverrideCarry::default(),
        )
        .await
    }

    /// Issue #368 Phase B: PATCH carry for per-target overrides.
    /// `existing_overrides` maps `endpoint_id` to the stored override
    /// (`None` means inherit). When the request omits `proxy_url_override`
    /// (`None`), the stored value is kept; `Some("")` clears to inherit;
    /// `Some(url)` replaces after scheme validation.
    /// Issue #378 Phase I: same omit-when-untouched carry applies to
    /// `active_windows` via `existing_windows` (`None` means all-day).
    /// Issue #1032: same carry applies to `embedding_dimensions_override`
    /// via `existing_dimensions`; an omitted field keeps the stored value.
    pub async fn into_create_with_existing(
        self,
        state: &AdminState,
        existing_overrides: &std::collections::HashMap<Uuid, Option<String>>,
        existing_windows: &std::collections::HashMap<Uuid, Vec<db::ActiveWindow>>,
        existing_dimensions: &DimensionOverrideCarry,
    ) -> Result<db::ModelEndpointRuleCreate, ApiError> {
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
                        // Issue #409 Phase 1: legacy path defaults to `Auto`.
                        native_api: None,
                        proxy_url_override: None,
                        has_proxy_url_override: None,
                        active_windows: None,
                        // Issue #392 Phase K: default-off passthrough.
                        dev_system_normalize: false,
                        // Issue #566: default-off thinking adaptation.
                        thinking_downgrade_enabled: false,
                        // Issue #464: inherit (follow the caller).
                        thinking_effort_override: None,
                        compact_mode: None,
                        // Issue #637: inherit the endpoint override.
                        service_tier: None,
                        // Issue #1032: omitted; no existing target to carry.
                        embedding_dimensions_override: None,
                    })
                    .collect()
            })
            .into_iter()
            .map(|target| async move {
                db::get_endpoint(&state.pool, target.endpoint_id)
                    .await
                    .map_err(|err| ApiError::internal(state, err))?
                    .ok_or_else(|| {
                        ApiError::new(
                            StatusCode::BAD_REQUEST,
                            "invalid_target_endpoint",
                            "target endpoint not found",
                        )
                    })?;
                let proxy_url_override = match target.proxy_url_override.as_deref() {
                    None => existing_overrides
                        .get(&target.endpoint_id)
                        .cloned()
                        .unwrap_or(None),
                    Some(raw) if raw.trim().is_empty() => None,
                    Some(raw) => Some(normalize_proxy_url(raw.trim()).map_err(|message| {
                        ApiError::new(StatusCode::BAD_REQUEST, "invalid_proxy_url", message)
                    })?),
                };
                let active_windows = match target.active_windows {
                    None => existing_windows.get(&target.endpoint_id).cloned(),
                    Some(windows) => {
                        Some(db::normalize_request_windows(&windows).map_err(|message| {
                            ApiError::new(
                                StatusCode::BAD_REQUEST,
                                "invalid_active_windows",
                                message,
                            )
                        })?)
                    }
                };
                // Issue #464: always sent (`None`/empty means inherit);
                // non-allowlist values reject with `invalid_thinking_effort`.
                let thinking_effort_override = db::normalize_thinking_effort_override(
                    target.thinking_effort_override.as_deref(),
                )
                .map_err(|message| {
                    ApiError::new(StatusCode::BAD_REQUEST, "invalid_thinking_effort", message)
                })?;
                // Issue #502 Task 5: always sent; unknown/empty normalizes
                // to `passthrough` (no new default semantics, no reject).
                let compact_mode = db::normalize_compact_mode(target.compact_mode.as_deref());
                // Issue #1032: omitted keeps the stored value; explicit null
                // clears to inherit; an integer must be a positive i32 and
                // is rejected with a 400 otherwise (never clamped).
                let upstream_model =
                    normalize_upstream_model(target.upstream_model.as_deref());
                let embedding_dimensions_override = match target.embedding_dimensions_override {
                    None => match existing_dimensions
                        .lookup(target.endpoint_id, upstream_model.as_deref())
                    {
                        DimensionCarryLookup::Value(value) => value,
                        DimensionCarryLookup::Absent => None,
                        DimensionCarryLookup::Ambiguous => {
                            return Err(ApiError::new(
                                StatusCode::BAD_REQUEST,
                                "ambiguous_embedding_dimensions_override",
                                "multiple stored targets share this endpoint and normalized upstream model with different embedding dimension overrides; send an explicit dimensions value or null",
                            ))
                        }
                    },
                    Some(None) => None,
                    Some(Some(value)) => db::normalize_embedding_dimensions_override(Some(value))
                        .map_err(|message| {
                        ApiError::new(
                            StatusCode::BAD_REQUEST,
                            "invalid_embedding_dimensions",
                            message,
                        )
                    })?,
                };
                Ok(db::ModelRouteTargetCreate {
                    endpoint_id: target.endpoint_id,
                    enabled: target.enabled.unwrap_or(true),
                    upstream_model,
                    // Issue #409 Phase 1: target port type, default `Auto`
                    // (always sent, no omit/carry like `dev_system_normalize`).
                    native_api: target.native_api.unwrap_or(crate::config::NativeApi::Auto),
                    proxy_url_override,
                    active_windows,
                    // Issue #392 Phase K: always sent (no omit/carry).
                    dev_system_normalize: target.dev_system_normalize,
                    // Issue #566: always sent (no omit/carry).
                    thinking_downgrade_enabled: target.thinking_downgrade_enabled,
                    thinking_effort_override,
                    compact_mode,
                    // Issue #637: always sent; blank normalizes to inherit.
                    service_tier: db::normalize_service_tier(target.service_tier.as_deref()),
                    // Issue #1032: omitted carries the stored value; null
                    // clears; a positive i32 sets the forced dimensions.
                    embedding_dimensions_override,
                })
            });
        let targets = futures::future::try_join_all(targets).await?;
        Ok(db::ModelEndpointRuleCreate {
            scope: self.scope,
            owner_user_id: self.owner_user_id,
            model_pattern: self.model_pattern,
            routing_strategy: self.routing_strategy.unwrap_or_default(),
            enabled: self.enabled.unwrap_or(true),
            targets,
        })
    }

    async fn validate(
        &self,
        state: &AdminState,
        existing_rule_id: Option<Uuid>,
    ) -> Result<(), ApiError> {
        let pattern = self.model_pattern.trim();
        if pattern.is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_model_pattern",
                "model pattern is required",
            ));
        }
        if !matches!(self.scope.as_str(), "admin" | "user") {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                "scope must be admin or user",
            ));
        }
        if self.scope == "admin" && self.owner_user_id.is_some() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "admin route cannot have owner",
            ));
        }
        if self.scope == "user" && self.owner_user_id.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "user route requires owner",
            ));
        }
        // Issue #466: single target + multi-key is the pinned key-level
        // stickiness scenario; no minimum-target check on the strategy.
        let targets = self.targets.as_deref().unwrap_or(&[]);
        let has_legacy_endpoint = self.endpoint_id.is_some();
        if targets.is_empty() && !has_legacy_endpoint {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_targets",
                "model route requires at least one target",
            ));
        }
        // Issue #419: identity is `(endpoint_id, normalized upstream_model)`,
        // so one upstream may serve several distinct models while a repeated
        // pair (empty/omitted both mean inherit) stays rejected.
        let target_keys = if targets.is_empty() {
            self.endpoint_id
                .iter()
                .copied()
                .map(|endpoint_id| target_dedup_key(endpoint_id, None))
                .collect::<Vec<_>>()
        } else {
            targets
                .iter()
                .map(|target| {
                    target_dedup_key(target.endpoint_id, target.upstream_model.as_deref())
                })
                .collect::<Vec<_>>()
        };
        if has_duplicate_targets(&target_keys) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "duplicate_targets",
                "model route targets must be unique per endpoint and upstream model",
            ));
        }
        let unique_target_ids = target_keys
            .iter()
            .map(|(endpoint_id, _)| *endpoint_id)
            .collect::<std::collections::HashSet<_>>();
        for endpoint_id in unique_target_ids {
            let endpoint = db::get_endpoint(&state.pool, endpoint_id)
                .await
                .map_err(|err| ApiError::internal(state, err))?;
            if endpoint.is_none() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_target_endpoint",
                    "target endpoint not found",
                ));
            }
        }
        // Issue #368 Phase B: scheme whitelist for per-target overrides.
        // Empty means clear (inherit); non-empty must be http/https/socks5/socks5h.
        for target in targets {
            if let Some(raw) = target.proxy_url_override.as_deref()
                && !raw.trim().is_empty()
                && let Err(message) = normalize_proxy_url(raw.trim())
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_proxy_url",
                    message,
                ));
            }
            // Issue #378 Phase I: HH:MM format, ranges, start != end;
            // end < start is overnight; overlaps allowed; sorted normalize.
            if let Some(windows) = target.active_windows.as_deref()
                && let Err(message) = db::normalize_request_windows(windows)
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_active_windows",
                    message,
                ));
            }
            // Issue #464: allowlist for per-target thinking effort.
            // Empty/`None` means inherit; non-allowlist rejects.
            if let Err(message) =
                db::normalize_thinking_effort_override(target.thinking_effort_override.as_deref())
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_thinking_effort",
                    message,
                ));
            }
            // Issue #1032: the explicit dimension override must be a positive
            // i32; omitted/null is inherit and never rejected.
            if let Some(Some(value)) = target.embedding_dimensions_override
                && let Err(message) = db::normalize_embedding_dimensions_override(Some(value))
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_embedding_dimensions",
                    message,
                ));
            }
        }
        let rules = db::list_model_endpoint_rules(&state.pool)
            .await
            .map_err(|err| ApiError::internal(state, err))?;
        if rules.iter().any(|rule| {
            Some(rule.rule_id) != existing_rule_id
                && rule.scope == self.scope
                && rule.owner_user_id == self.owner_user_id
                && rule.model_pattern == pattern
        }) {
            return Err(ApiError::new(
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
    /// Issue #409 Phase 1: per-target port type. `None` (omitted/null)
    /// means `Auto` (follow the caller); an explicit value wins over the
    /// upstream endpoint `native_api`.
    #[serde(default)]
    pub native_api: Option<crate::config::NativeApi>,
    /// Issue #368 Phase B: per-target proxy override. `None` (omitted/null)
    /// means keep on PATCH / inherit on create; `Some("")` means clear to
    /// inherit; `Some(url)` must use `http/https/socks5/socks5h`.
    #[serde(default)]
    pub proxy_url_override: Option<String>,
    /// Issue #368 Phase B: carry hint for the override secret. Accepted for
    /// forward-compat; the server ignores it and uses the stored value when
    /// `proxy_url_override` is omitted.
    #[serde(default)]
    pub has_proxy_url_override: Option<bool>,
    /// Issue #378 Phase I: effective windows (`[{start,end}]` `HH:MM`).
    /// `None` (omitted) keeps the stored value on PATCH; `Some([])`
    /// means all-day; `Some([...])` replaces after validation.
    #[serde(default)]
    pub active_windows: Option<Vec<db::ActiveWindow>>,
    /// Issue #392 Phase K: developer->system normalization switch.
    /// Always sent (no omit semantics); `false` (default) skips
    /// `normalize_chat_request_for_native` in Chat passthrough.
    #[serde(default)]
    pub dev_system_normalize: bool,
    /// Issue #566: per-target thinking adaptation switch (pre-flight
    /// downgrade + reasoning-echo fingerprint retry). Always sent (no omit
    /// semantics); `false` (default) keeps the pre-#556 byte-identical
    /// passthrough while the env escape hatch still forces a full bypass.
    #[serde(default)]
    pub thinking_downgrade_enabled: bool,
    /// Issue #464: per-target thinking effort override. `None`
    /// (omitted/null/empty) means inherit (follow the caller);
    /// `Some(effort)` must be one of
    /// `none/minimal/low/medium/high/xhigh/max` and force-replaces the
    /// caller value on Chat (`reasoning_effort`), Responses
    /// (`reasoning.effort`), and Anthropic Messages
    /// (`output_config.effort`). Always sent (no omit/carry); `None` clears
    /// to inherit.
    #[serde(default)]
    pub thinking_effort_override: Option<String>,
    /// Issue #502 Task 5: per-target compact mode. `None`
    /// (omitted/null/empty) means `passthrough` (no new default
    /// semantics); `Some("self_summarize")` enables ferry-side handoff
    /// summarization for non-Responses targets; `Some("off")` rejects
    /// compact explicitly. Always sent (no omit/carry); unknown values
    /// normalize to `passthrough`.
    #[serde(default)]
    pub compact_mode: Option<String>,
    /// Issue #637: per-target free-form service-tier override. `None`
    /// (omitted/null/empty) means inherit the endpoint value; `Some(value)`
    /// is trimmed and wins over the endpoint override.
    #[serde(default)]
    pub service_tier: Option<String>,
    /// Issue #1032: per-target embedding dimension override. The field is
    /// `Option<Option<i32>>` so an omitted field (`None`) carries the stored
    /// value on PATCH, an explicit `null` (`Some(None)`) clears it to
    /// inherit, and an integer (`Some(Some(value))`) sets a positive i32.
    /// Omitting must never clear a saved dimension.
    #[serde(default, deserialize_with = "deserialize_nullable_override")]
    #[schema(minimum = 1)]
    pub embedding_dimensions_override: Option<Option<i32>>,
}

/// Issue #1032: distinguish an omitted field from an explicit `null` for a
/// nullable numeric override. `serde` would otherwise deserialize both as
/// `None`, which would silently clear a stored dimension on PATCH.
fn deserialize_nullable_override<'de, D>(deserializer: D) -> Result<Option<Option<i32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<i32>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ModelRouteTestRequest {
    pub rule_id: Uuid,
}

/// Issue #1004 P2: the outcome of one model-route target.
///
/// The target identity is preserved (`target_id` plus the endpoint id and
/// name), so two targets of the same endpoint stay separate outcomes. The
/// message never carries a credential.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelRouteTestTargetResult {
    pub target_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_name: String,
    pub model: Option<String>,
    pub ok: bool,
    pub status: Option<u16>,
    #[schema(value_type = u64)]
    pub duration_ms: u128,
    pub message: String,
    /// The preferred target of the rule. It is metadata for the reader, never
    /// a filter: every eligible target is tested.
    pub preferred: bool,
}

/// The model-route test result.
///
/// The top-level fields describe one representative outcome (the preferred
/// target's outcome when it was tested, otherwise the first outcome) so a
/// client that only knows the single-result shape keeps working. `results`
/// carries every eligible target's outcome in the configured order, `passed`
/// and `total` the aggregate counts, and `ok` is true only when every tested
/// target passed.
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
    pub results: Vec<ModelRouteTestTargetResult>,
    pub passed: u32,
    pub total: u32,
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

/// Issue #419: trim an upstream model and map whitespace-only/empty to
/// `None` (inherit). Shared by persistence (`into_create`) and target
/// dedup so validation and stored rows agree on what "same target" means.
fn normalize_upstream_model(upstream_model: Option<&str>) -> Option<String> {
    upstream_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Issue #419: dedup identity for a route target.
fn target_dedup_key(endpoint_id: Uuid, upstream_model: Option<&str>) -> (Uuid, Option<String>) {
    (endpoint_id, normalize_upstream_model(upstream_model))
}

/// Issue #419: `true` when two targets share the same endpoint and the same
/// normalized upstream model.
fn has_duplicate_targets(keys: &[(Uuid, Option<String>)]) -> bool {
    keys.iter().collect::<std::collections::HashSet<_>>().len() != keys.len()
}

/// Issue #1032: stored per-target dimension overrides for a PATCH that omits
/// the field.
///
/// The request can only name a target by `endpoint_id` plus `upstream_model`,
/// and the endpoint+model pair is not unique in storage (the primary key is
/// `target_id`). The carry is therefore keyed by the same canonical identity
/// the request expresses: endpoint id plus the normalized upstream model.
/// Distinct models on one endpoint keep independent values, and an existing
/// pair with conflicting stored values is reported as ambiguous instead of
/// being copied arbitrarily.
#[derive(Debug, Default, Clone)]
pub struct DimensionOverrideCarry {
    values: std::collections::HashMap<(Uuid, Option<String>), Option<i32>>,
    ambiguous: std::collections::HashSet<(Uuid, Option<String>)>,
}

/// The carry answer for one requested target identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionCarryLookup {
    /// No stored target has this identity; the value stays inherit.
    Absent,
    /// Every matching stored target agrees on this value (which may be inherit).
    Value(Option<i32>),
    /// Matching stored targets disagree; the caller must be explicit.
    Ambiguous,
}

impl DimensionOverrideCarry {
    /// Build the carry from stored targets. `upstream_model` is normalized with
    /// the same rule the request path uses.
    pub fn from_targets(
        targets: impl IntoIterator<Item = (Uuid, Option<String>, Option<i32>)>,
    ) -> Self {
        let mut grouped: std::collections::HashMap<
            (Uuid, Option<String>),
            std::collections::HashSet<Option<i32>>,
        > = std::collections::HashMap::new();
        for (endpoint_id, upstream_model, dimensions) in targets {
            grouped
                .entry(target_dedup_key(endpoint_id, upstream_model.as_deref()))
                .or_default()
                .insert(dimensions);
        }
        let mut carry = Self::default();
        for (key, values) in grouped {
            if values.len() == 1 {
                if let Some(value) = values.into_iter().next() {
                    carry.values.insert(key, value);
                }
            } else {
                carry.ambiguous.insert(key);
            }
        }
        carry
    }

    fn lookup(&self, endpoint_id: Uuid, upstream_model: Option<&str>) -> DimensionCarryLookup {
        let key = target_dedup_key(endpoint_id, upstream_model);
        if self.ambiguous.contains(&key) {
            return DimensionCarryLookup::Ambiguous;
        }
        match self.values.get(&key) {
            Some(value) => DimensionCarryLookup::Value(*value),
            None => DimensionCarryLookup::Absent,
        }
    }
}

/// Issue #368 Phase B: normalize and validate an outbound proxy URL.
/// Empty is handled by callers (clear to inherit/direct); this helper only
/// validates non-empty values. Returns the trimmed URL on success without
/// echoing userinfo in errors.
fn normalize_proxy_url(trimmed: &str) -> Result<String, &'static str> {
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "proxy_url scheme must be one of http, https, socks5, socks5h")?;
    match parsed.scheme().to_ascii_lowercase().as_str() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => return Err("proxy_url scheme must be one of http, https, socks5, socks5h"),
    }
    if parsed.host_str().is_none_or(|host| host.trim().is_empty()) {
        return Err("proxy_url must include a host");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ModelRouteTargetRequest, has_duplicate_targets, target_dedup_key};

    #[test]
    fn target_dedup_allows_same_endpoint_different_model() {
        // Issue #419: one upstream may serve several distinct models.
        let endpoint = uuid::Uuid::nil();
        let keys = vec![
            target_dedup_key(endpoint, Some("deepseek-flash")),
            target_dedup_key(endpoint, Some("muse-spark-1.3-contributor")),
        ];
        assert!(!has_duplicate_targets(&keys));
    }

    #[test]
    fn target_dedup_rejects_same_endpoint_same_model() {
        let endpoint = uuid::Uuid::nil();
        let keys = vec![
            target_dedup_key(endpoint, Some("deepseek-flash")),
            target_dedup_key(endpoint, Some("deepseek-flash")),
        ];
        assert!(has_duplicate_targets(&keys));
    }

    #[test]
    fn target_dedup_rejects_empty_and_inherited_duplicates() {
        let endpoint = uuid::Uuid::nil();
        // Empty string and omitted both mean inherit.
        let empty_and_none = vec![
            target_dedup_key(endpoint, Some("")),
            target_dedup_key(endpoint, None),
        ];
        assert!(has_duplicate_targets(&empty_and_none));
        let both_none = vec![
            target_dedup_key(endpoint, None),
            target_dedup_key(endpoint, None),
        ];
        assert!(has_duplicate_targets(&both_none));
        let both_empty = vec![
            target_dedup_key(endpoint, Some("")),
            target_dedup_key(endpoint, Some("")),
        ];
        assert!(has_duplicate_targets(&both_empty));
    }

    #[test]
    fn target_dedup_trims_whitespace() {
        // Issue #419: normalization matches persistence (`trim`, blank ->
        // inherit), so padded and bare values collide.
        let endpoint = uuid::Uuid::nil();
        let padded = vec![
            target_dedup_key(endpoint, Some("  deepseek-flash  ")),
            target_dedup_key(endpoint, Some("deepseek-flash")),
        ];
        assert!(has_duplicate_targets(&padded));
        let blank = vec![
            target_dedup_key(endpoint, Some("   ")),
            target_dedup_key(endpoint, None),
        ];
        assert!(has_duplicate_targets(&blank));
        // Distinct endpoints never collide even with the same model.
        let distinct = vec![
            target_dedup_key(uuid::Uuid::from_u128(1), Some("deepseek-flash")),
            target_dedup_key(uuid::Uuid::from_u128(2), Some("deepseek-flash")),
        ];
        assert!(!has_duplicate_targets(&distinct));
    }

    #[test]
    fn dev_system_normalize_defaults_off_and_always_serializes() {
        // Issue #392 Phase K: booleans always sent (no omit semantics).
        // Omitted input defaults to false for backward compat, but
        // serialization always carries the key so `false` means off.
        // Issue #566 mirrors this contract for `thinking_downgrade_enabled`.
        let omitted: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
            "endpoint_id": "00000000-0000-0000-0000-000000000000"
        }))
        .expect("missing normalize defaults to false");
        assert!(!omitted.dev_system_normalize);
        // Issue #566: the thinking adaptation switch defaults off too.
        assert!(!omitted.thinking_downgrade_enabled);
        // Issue #409 Phase 1: omitted target port type defaults to `Auto`.
        assert_eq!(omitted.native_api, None);
        let off = serde_json::to_value(&ModelRouteTargetRequest {
            endpoint_id: uuid::Uuid::nil(),
            enabled: Some(true),
            upstream_model: None,
            native_api: None,
            proxy_url_override: None,
            has_proxy_url_override: None,
            active_windows: None,
            dev_system_normalize: false,
            thinking_downgrade_enabled: false,
            thinking_effort_override: None,
            compact_mode: None,
            service_tier: None,
            embedding_dimensions_override: None,
        })
        .expect("serialize off");
        assert_eq!(
            off.get("dev_system_normalize").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            off.get("thinking_downgrade_enabled")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        let on = serde_json::to_value(&ModelRouteTargetRequest {
            endpoint_id: uuid::Uuid::nil(),
            enabled: Some(true),
            upstream_model: None,
            native_api: Some(crate::config::NativeApi::Chat),
            proxy_url_override: None,
            has_proxy_url_override: None,
            active_windows: None,
            dev_system_normalize: true,
            thinking_downgrade_enabled: true,
            thinking_effort_override: Some("high".to_string()),
            // Issue #502 Task 5: compact mode round-trips; omitted means
            // passthrough.
            compact_mode: Some("self_summarize".to_string()),
            service_tier: Some("fast".to_string()),
            embedding_dimensions_override: None,
        })
        .expect("serialize on");
        assert_eq!(
            on.get("dev_system_normalize").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            on.get("thinking_downgrade_enabled")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn target_native_api_omitted_means_auto() {
        // Issue #409 Phase 1: `None` (omitted/null) maps to `Auto`
        // (follow the caller) in `into_create` paths.
        let omitted: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
            "endpoint_id": "00000000-0000-0000-0000-000000000000"
        }))
        .expect("parse");
        assert_eq!(omitted.native_api, None);
        let explicit: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
            "endpoint_id": "00000000-0000-0000-0000-000000000000",
            "native_api": "chat"
        }))
        .expect("parse explicit");
        assert_eq!(explicit.native_api, Some(crate::config::NativeApi::Chat));
    }

    #[test]
    fn thinking_effort_override_inherit_and_allowlist() {
        // Issue #464: `None` (omitted/null/empty) means inherit (follow
        // the caller); explicit values must hit the 7-level allowlist.
        let omitted: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
            "endpoint_id": "00000000-0000-0000-0000-000000000000"
        }))
        .expect("missing effort means inherit");
        assert_eq!(omitted.thinking_effort_override, None);
        assert!(
            crate::db::normalize_thinking_effort_override(Some(" high ")).unwrap()
                == Some("high".to_string())
        );
        assert!(
            crate::db::normalize_thinking_effort_override(Some(""))
                .unwrap()
                .is_none()
        );
        assert!(
            crate::db::normalize_thinking_effort_override(None)
                .unwrap()
                .is_none()
        );
        assert!(crate::db::normalize_thinking_effort_override(Some("ultra")).is_err());
    }

    #[test]
    fn routing_strategy_defaults_to_affinity_allowing_single_target() {
        // Issue #466: the removed legacy variant leaves a
        // single-variant enum; the default is affinity and single-target
        // routes (key-level stickiness, e.g. `minimax-m3`) are valid.
        assert_eq!(
            crate::db::ModelRouteRoutingStrategy::default(),
            crate::db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
        );
        let omitted: super::ModelRouteRequest = serde_json::from_value(serde_json::json!({
            "scope": "admin",
            "model_pattern": "minimax-m3",
            "targets": [{
                "endpoint_id": "00000000-0000-0000-0000-000000000000"
            }]
        }))
        .expect("parse single-target route");
        assert_eq!(
            omitted.routing_strategy.unwrap_or_default(),
            crate::db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
        );
        assert_eq!(omitted.targets.map(|targets| targets.len()), Some(1));
    }
}
