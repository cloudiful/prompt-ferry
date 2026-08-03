use super::*;

pub async fn list_model_endpoint_rules(pool: &PgPool) -> Result<Vec<ModelEndpointRule>> {
    let rows = sqlx::query_file_as!(
        ModelEndpointRuleRow,
        "src/sql/routes/list_model_endpoint_rules.sql",
    )
    .fetch_all(pool)
    .await?;
    hydrate::hydrate_rules(pool, rows).await
}

pub async fn list_model_endpoint_rules_page(
    pool: &PgPool,
    first: i64,
    rows: i64,
) -> Result<ModelRoutePage> {
    let total = sqlx::query_file!("src/sql/routes/count_model_endpoint_rules.sql")
        .fetch_one(pool)
        .await?
        .total;
    let route_rows = sqlx::query_file_as!(
        ModelEndpointRuleRow,
        "src/sql/routes/list_model_endpoint_rules_page.sql",
        first.max(0),
        rows.clamp(1, 200),
    )
    .fetch_all(pool)
    .await?;
    Ok(ModelRoutePage {
        total,
        routes: hydrate::hydrate_rules(pool, route_rows).await?,
        first: first.max(0),
        rows: rows.clamp(1, 200),
    })
}

pub async fn get_model_endpoint_rule(
    pool: &PgPool,
    rule_id: uuid::Uuid,
) -> Result<Option<ModelEndpointRule>> {
    let row = sqlx::query_file_as!(
        ModelEndpointRuleRow,
        "src/sql/routes/get_model_endpoint_rule.sql",
        rule_id,
    )
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => Ok(hydrate::hydrate_rules(pool, vec![row])
            .await?
            .into_iter()
            .next()),
        None => Ok(None),
    }
}

pub async fn get_model_route_candidate(
    pool: &PgPool,
    rule_id: uuid::Uuid,
) -> Result<Option<ModelRouteCandidate>> {
    let candidates = hydrate::model_route_candidates_by_rule(pool, Some(rule_id), None).await?;
    Ok(candidates.into_iter().next())
}

pub async fn model_route_candidates(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<ModelRouteCandidate>> {
    hydrate::model_route_candidates_by_rule(pool, None, Some(user_id)).await
}

pub async fn list_visible_model_route_endpoints(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<RouteConfig>> {
    let mut routes = list_visible_model_route_endpoints_strict(pool, user_id).await?;

    if routes.is_empty()
        && let Some(route) = snapshot::effective_route(pool, user_id).await?
    {
        routes.push(route);
    }
    Ok(routes)
}

pub async fn list_visible_model_route_endpoints_strict(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<RouteConfig>> {
    let rows = sqlx::query_file!(
        "src/sql/routes/list_visible_model_route_endpoints.sql",
        user_id,
    )
    .fetch_all(pool)
    .await?;

    let routes = rows
        .into_iter()
        .map(|row| RouteConfig {
            route_id: row.endpoint_id,
            user_id,
            model_route_rule_id: None,
            base_url: row.base_url,
            api_key: row.api_key,
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: row.key_lb_enabled,
            native_api: parse_native_api(&row.native_api),
            upstream_model: None,
            responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            chat_reasoning_replay_policy: crate::db::ChatReasoningReplayPolicy::Auto,
            route_selection_reason: crate::db::RouteSelectionReason::Default,
        })
        .collect::<Vec<_>>();
    crate::db::endpoints::attach_route_config_api_keys(pool, routes).await
}
