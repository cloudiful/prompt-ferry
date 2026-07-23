use super::*;

pub async fn effective_route(pool: &PgPool, user_id: i64) -> Result<Option<RouteConfig>> {
    let route = sqlx::query_file!("src/sql/routes/effective_route.sql", user_id,)
        .fetch_optional(pool)
        .await?
        .map(|row| RouteConfig {
            route_id: row.route_id,
            user_id: row.user_id,
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
            route_selection_reason: crate::db::RouteSelectionReason::Default,
        });
    match route {
        Some(route) => Ok(
            crate::db::endpoints::attach_route_config_api_keys(pool, vec![route])
                .await?
                .into_iter()
                .next(),
        ),
        None => Ok(None),
    }
}

pub async fn resolve_model_route(
    pool: &PgPool,
    user_id: i64,
    model: Option<&str>,
) -> Result<(Option<RouteConfig>, Option<ModelRouteCandidate>)> {
    resolve_model_route_with_fallback(pool, user_id, model, true).await
}

pub async fn resolve_model_route_with_fallback(
    pool: &PgPool,
    user_id: i64,
    model: Option<&str>,
    allow_fallback: bool,
) -> Result<(Option<RouteConfig>, Option<ModelRouteCandidate>)> {
    if let Some(model) = model {
        let candidate = queries::model_route_candidates(pool, user_id)
            .await?
            .into_iter()
            .filter_map(|candidate| {
                matching::model_pattern_specificity(&candidate.model_pattern, model)
                    .map(|specificity| (candidate, specificity))
            })
            .max_by(
                |(left_candidate, left_specificity), (right_candidate, right_specificity)| {
                    matching::route_precedence_key(left_candidate, *left_specificity).cmp(
                        &matching::route_precedence_key(right_candidate, *right_specificity),
                    )
                },
            )
            .map(|(candidate, _)| candidate);
        if let Some(candidate) = candidate {
            return Ok((None, Some(candidate)));
        }
        if !allow_fallback {
            return Ok((None, None));
        }
    }
    Ok((effective_route(pool, user_id).await?, None))
}

pub async fn snapshot_keys(pool: &PgPool) -> Result<Vec<SnapshotKey>> {
    let keys = sqlx::query_file_as!(ClientKeyRow, "src/sql/routes/snapshot_keys.sql",)
        .fetch_all(pool)
        .await?;

    let mut snapshot = Vec::with_capacity(keys.len());
    for key in keys {
        if let Some(route_id) = snapshot_route_id_for_user(pool, key.user_id).await? {
            snapshot.push(SnapshotKey {
                key_hash: key.key_hash,
                key_prefix: key.key_prefix,
                user_id: key.user_id,
                route_id,
            });
        }
    }
    Ok(snapshot)
}

async fn snapshot_route_id_for_user(pool: &PgPool, user_id: i64) -> Result<Option<uuid::Uuid>> {
    if let Some(route) = effective_route(pool, user_id).await? {
        return Ok(Some(route.route_id));
    }
    let candidates = queries::model_route_candidates(pool, user_id).await?;
    for candidate in candidates {
        if let Some(target) = candidate.targets.first() {
            return Ok(Some(target.endpoint_id));
        }
    }
    Ok(None)
}
