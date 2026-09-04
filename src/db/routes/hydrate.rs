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
        .map(|row| ModelRouteTarget {
            target_id: row.target_id,
            rule_id: row.rule_id,
            endpoint_id: row.endpoint_id,
            endpoint_name: row.endpoint_name,
            endpoint_enabled: row.endpoint_enabled,
            position: row.position,
            enabled: row.enabled,
            upstream_model: row.upstream_model,
            responses_continuation_policy: parse_responses_continuation_policy(
                &row.responses_continuation_policy,
            ),
            created_at: row.created_at,
            updated_at: row.updated_at,
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
        if let Some(candidate) = grouped
            .iter_mut()
            .find(|candidate| candidate.rule_id == row.rule_id)
        {
            candidate.targets.push(ModelRouteCandidateTarget {
                target_id: row.target_id,
                endpoint_id: row.endpoint_id,
                endpoint_name: row.endpoint_name,
                base_url: row.base_url,
                api_key: row.api_key,
                api_keys,
                key_lb_enabled: row.key_lb_enabled,
                native_api: parse_native_api(&row.native_api),
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model,
                responses_continuation_policy: parse_responses_continuation_policy(
                    &row.responses_continuation_policy,
                ),
                provider,
                service_tier,
            });
            continue;
        }
        grouped.push(ModelRouteCandidate {
            rule_id: row.rule_id,
            scope: row.scope,
            owner_user_id: row.owner_user_id,
            model_pattern: row.model_pattern,
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
                native_api: parse_native_api(&row.native_api),
                position: row.position,
                enabled: row.target_enabled,
                upstream_model: row.upstream_model,
                responses_continuation_policy: parse_responses_continuation_policy(
                    &row.responses_continuation_policy,
                ),
                provider,
                service_tier,
            }],
        });
    }
    Ok(grouped)
}
