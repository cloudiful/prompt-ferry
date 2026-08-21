use super::*;
use std::collections::HashSet;

pub(super) async fn list_client_keys(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match db::list_client_keys_page(&state.pool, user.user_id, first, rows).await {
        Ok((total, keys)) => Json(ClientKeyPageResponse {
            keys,
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CreateClientKeyRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let (secret, prefix, hash) = generate_client_key();
    match db::create_client_key(
        &state.pool,
        user.user_id,
        body.label.as_deref().unwrap_or("Codex key"),
        &prefix,
        &hash,
        &secret,
    )
    .await
    {
        Ok(key) => {
            let _ = publish_snapshot(&state).await;
            Json(CreateClientKeyResponse {
                key_id: key.key_id,
                user_id: key.user_id,
                key_prefix: key.key_prefix,
                label: key.label,
                enabled: key.enabled,
                created_at: key.created_at,
                secret,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(key_id): Path<i64>,
    Json(body): Json<UpdateClientKeyRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    match db::update_client_key(&state.pool, user.user_id, key_id, body.label, body.enabled).await {
        Ok(Some(key)) => {
            let _ = publish_snapshot(&state).await;
            Json(key).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(key_id): Path<i64>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    match db::delete_client_key(&state.pool, user.user_id, key_id).await {
        Ok(true) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn list_available_models(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match available_models(&state, user.user_id).await {
        Ok(models) => {
            let total = i64::try_from(models.len()).unwrap_or(i64::MAX);
            let models = models
                .into_iter()
                .skip(usize::try_from(first).unwrap_or(usize::MAX))
                .take(usize::try_from(rows).unwrap_or(usize::MAX))
                .collect();
            Json(AvailableModelsResponse {
                models,
                total,
                first,
                rows,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

async fn available_models(state: &AdminState, user_id: i64) -> anyhow::Result<Vec<AvailableModel>> {
    let whitelist_enabled = state
        .model_route_whitelist_enabled
        .load(std::sync::atomic::Ordering::SeqCst);
    let client = reqwest::Client::new();
    let mut models = Vec::new();
    let mut seen = HashSet::<String>::new();

    if whitelist_enabled {
        for candidate in db::model_route_candidates(&state.pool, user_id).await? {
            if !candidate.model_pattern.contains('*') {
                if seen.insert(candidate.model_pattern.clone()) {
                    models.push(AvailableModel {
                        name: candidate.model_pattern.clone(),
                        id: candidate.model_pattern,
                    });
                }
                continue;
            }

            for target in candidate.targets {
                let route = db::RouteConfig {
                    route_id: target.endpoint_id,
                    user_id,
                    model_route_rule_id: Some(candidate.rule_id),
                    base_url: target.base_url,
                    api_key: target.api_key,
                    endpoint_key_id: None,
                    endpoint_key_label: None,
                    api_keys: target.api_keys,
                    key_lb_enabled: target.key_lb_enabled,
                    native_api: target.native_api,
                    upstream_model: target.upstream_model,
                    responses_continuation_policy: target.responses_continuation_policy,
                    route_selection_reason: db::RouteSelectionReason::Default,
                };
                let snapshot = state
                    .endpoint_model_cache
                    .load_or_fetch(&route, &|route| {
                        let client = client.clone();
                        let route = route.clone();
                        async move {
                            crate::endpoint_models::fetch_endpoint_model_ids(&client, &route).await
                        }
                    })
                    .await?
                    .unwrap_or_default();
                for model_id in snapshot.model_ids().filter(|model_id| {
                    db::model_pattern_matches(&candidate.model_pattern, model_id)
                }) {
                    if seen.insert(model_id.to_string()) {
                        models.push(AvailableModel {
                            name: model_id.to_string(),
                            id: model_id.to_string(),
                        });
                    }
                }
            }
        }
    } else {
        for route in db::list_visible_endpoints(&state.pool, user_id).await? {
            let snapshot = state
                .endpoint_model_cache
                .load_or_fetch(&route, &|route| {
                    let client = client.clone();
                    let route = route.clone();
                    async move {
                        crate::endpoint_models::fetch_endpoint_model_ids(&client, &route).await
                    }
                })
                .await?
                .unwrap_or_default();
            for model_id in snapshot.model_ids() {
                if seen.insert(model_id.to_string()) {
                    models.push(AvailableModel {
                        name: model_id.to_string(),
                        id: model_id.to_string(),
                    });
                }
            }
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(models)
}
