use super::*;

pub(super) async fn hydrate_rules(
    pool: &PgPool,
    rows: Vec<ModelEndpointRuleRow>,
) -> Result<Vec<ModelEndpointRule>> {
    let rule_ids = rows.iter().map(|row| row.rule_id).collect::<Vec<_>>();
    let targets = load_targets(pool, &rule_ids).await?;
    Ok(rows
        .into_iter()
        .map(|row| ModelEndpointRule {
            rule_id: row.rule_id,
            scope: row.scope,
            owner_user_id: row.owner_user_id,
            model_pattern: row.model_pattern,
            routing_strategy: parse_routing_strategy(&row.routing_strategy),
            daily_max_requests: row.daily_max_requests,
            monthly_max_requests: row.monthly_max_requests,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
            targets: targets
                .iter()
                .filter(|target| target.rule_id == row.rule_id)
                .cloned()
                .collect(),
        })
        .collect())
}

async fn load_targets(pool: &PgPool, rule_ids: &[uuid::Uuid]) -> Result<Vec<ModelRouteTarget>> {
    if rule_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_file_as!(
        ModelRouteTargetRow,
        "src/sql/routes/load_targets.sql",
        rule_ids,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            // Issue #368 Phase C (P2): derive the saved-override indicator;
            // the secret itself stays `skip_serializing`.
            let has_proxy_url_override = row
                .proxy_url_override
                .as_deref()
                .is_some_and(|raw| !raw.trim().is_empty());
            // Issue #378 Phase I: stored JSON array; corrupt reads as
            // all-day for display (routing fails closed separately).
            let active_windows =
                active_windows::parse_stored_windows(row.active_windows.as_deref())
                    .unwrap_or_default();
            // Issue #409 Phase 1: per-target native API, default `auto`
            // (COALESCE in SQL already defaults pre-migration rows).
            let native_api = row
                .native_api
                .as_deref()
                .map(parse_native_api)
                .unwrap_or(NativeApi::Auto);
            ModelRouteTarget {
                target_id: row.target_id,
                rule_id: row.rule_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name,
                endpoint_enabled: row.endpoint_enabled,
                position: row.position,
                enabled: row.enabled,
                upstream_model: row.upstream_model,
                native_api,
                proxy_url_override: row.proxy_url_override,
                has_proxy_url_override,
                active_windows,
                // Issue #392 Phase K: normalize switch, default-off for
                // pre-migration rows (COALESCE in SQL already defaults).
                dev_system_normalize: row.dev_system_normalize,
                created_at: row.created_at,
                updated_at: row.updated_at,
            }
        })
        .collect())
}

pub(super) async fn model_route_candidates_by_rule(
    pool: &PgPool,
    rule_id: Option<uuid::Uuid>,
    user_id: Option<i64>,
) -> Result<Vec<ModelRouteCandidate>> {
    let rows = if let Some(rule_id) = rule_id {
        sqlx::query_file_as!(
            ModelRouteCandidateRow,
            "src/sql/routes/model_route_candidates_by_rule_id.sql",
            rule_id,
        )
        .fetch_all(pool)
        .await?
    } else {
        let user_id = user_id.ok_or_else(|| anyhow!("user_id is required"))?;
        sqlx::query_file_as!(
            ModelRouteCandidateRow,
            "src/sql/routes/model_route_candidates_by_user_id.sql",
            user_id,
        )
        .fetch_all(pool)
        .await?
    };
    let endpoint_api_keys = crate::db::endpoints::list_endpoint_api_keys_by_endpoint_id(
        pool,
        &rows.iter().map(|row| row.endpoint_id).collect::<Vec<_>>(),
    )
    .await?;

    let mut grouped = Vec::<ModelRouteCandidate>::new();
    for row in rows {
        let api_keys = endpoint_api_keys
            .get(&row.endpoint_id)
            .cloned()
            .filter(|keys| !keys.is_empty())
            .unwrap_or_else(|| {
                fallback_api_keys(row.endpoint_id, &row.endpoint_name, &row.api_key)
            });
        let provider = crate::db::EndpointProvider::from_str(&row.provider);
        let service_tier =
            crate::db::MinimaxServiceTier::from_optional(row.service_tier.as_deref());
        // Issue #409 Phase 1: target explicit wins, else endpoint fallback.
        // Both `Auto` stays `Auto` for per-caller `resolve_auto_protocol`.
        let target_native_api = row
            .target_native_api
            .as_deref()
            .map(parse_native_api)
            .unwrap_or(NativeApi::Auto);
        let endpoint_native_api = parse_native_api(&row.native_api);
        let native_api =
            crate::db::resolve_target_native_api(target_native_api, endpoint_native_api);
        if let Some(candidate) = grouped
            .iter_mut()
            .find(|candidate| candidate.rule_id == row.rule_id)
        {
            candidate.targets.push(ModelRouteCandidateTarget {
                target_id: row.target_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name.clone(),
                base_url: row.base_url.clone(),
                api_key: row.api_key.clone(),
                api_keys,
                key_lb_enabled: row.key_lb_enabled,
                native_api,
                target_native_api,
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model.clone(),
                provider,
                service_tier,
                proxy_url: row.proxy_url.clone(),
                proxy_url_override: row.proxy_url_override.clone(),
                active_windows: row.active_windows.clone(),
                endpoint_active_windows: row.endpoint_active_windows.clone(),
                dev_system_normalize: row.dev_system_normalize,
            });
            continue;
        }
        grouped.push(ModelRouteCandidate {
            rule_id: row.rule_id,
            scope: row.scope.clone(),
            owner_user_id: row.owner_user_id,
            model_pattern: row.model_pattern.clone(),
            routing_strategy: parse_routing_strategy(&row.routing_strategy),
            daily_max_requests: row.daily_max_requests,
            monthly_max_requests: row.monthly_max_requests,
            updated_at: row.updated_at,
            targets: vec![ModelRouteCandidateTarget {
                target_id: row.target_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name,
                base_url: row.base_url,
                api_key: row.api_key,
                api_keys,
                key_lb_enabled: row.key_lb_enabled,
                native_api,
                target_native_api,
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model,
                provider,
                service_tier,
                proxy_url: row.proxy_url,
                proxy_url_override: row.proxy_url_override,
                active_windows: row.active_windows,
                endpoint_active_windows: row.endpoint_active_windows,
                dev_system_normalize: row.dev_system_normalize,
            }],
        });
    }
    Ok(grouped)
}
