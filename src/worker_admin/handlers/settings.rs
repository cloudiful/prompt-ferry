use super::*;

pub(super) async fn get_redaction_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RedactionSettingQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let (scope, user_id) = match resolve_redaction_scope(&user, query) {
        Ok(target) => target,
        Err(response) => return response.into_response(),
    };
    let config = match scope {
        RedactionScope::Global => match db::get_redaction_config(&state.pool).await {
            Ok(config) => config,
            Err(err) => return internal(&state, err),
        },
        RedactionScope::User => match db::get_user_redaction_config(
            &state.pool,
            user_id.expect("user redaction scope requires user id"),
        )
        .await
        {
            Ok(config) => config,
            Err(err) => return internal(&state, err),
        },
    };
    let config = config.normalized();
    Json(RedactionSettingResponse {
        scope,
        user_id,
        config,
    })
    .into_response()
}

pub(super) async fn set_redaction_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RedactionSettingQuery>,
    Json(body): Json<RedactionSettingRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let (scope, user_id) = match resolve_redaction_scope(&user, query) {
        Ok(target) => target,
        Err(response) => return response.into_response(),
    };
    let previous = match scope {
        RedactionScope::Global => db::get_redaction_config(&state.pool).await,
        RedactionScope::User => {
            db::get_user_redaction_config(
                &state.pool,
                user_id.expect("user redaction scope requires user id"),
            )
            .await
        }
    };
    let previous = match previous {
        Ok(config) => config,
        Err(err) => return internal(&state, err),
    };
    let previous_blob_updated_at = match db::get_redaction_config_updated_at(
        &state.pool,
        matches!(scope, RedactionScope::Global),
        user_id,
    )
    .await
    {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };

    let mut config = body.0.normalized();
    stamp_custom_string_timestamps(
        &previous.normalized().custom_strings,
        previous_blob_updated_at,
        &mut config.custom_strings,
        Utc::now(),
    );
    if let Err(err) = config.validate() {
        return bad_request(&err.to_string());
    }
    let result = match scope {
        RedactionScope::Global => db::set_redaction_config(&state.pool, &config).await,
        RedactionScope::User => {
            db::set_user_redaction_config(
                &state.pool,
                user_id.expect("user redaction scope requires user id"),
                &config,
            )
            .await
        }
    };
    match result {
        Ok(()) => {
            let apply_result = match scope {
                RedactionScope::Global => redact::apply_config(&config),
                RedactionScope::User => redact::apply_user_config(
                    user_id.expect("user redaction scope requires user id"),
                    &config,
                ),
            };
            if let Err(err) = apply_result {
                return bad_request(&err.to_string());
            }
            state
                .redaction_enabled
                .store(redact::has_any_enabled(), Ordering::SeqCst);
            Json(RedactionSettingResponse {
                scope,
                user_id,
                config,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn preview_redaction(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<RedactionPreviewRequestBody>,
) -> Response {
    if let Err(response) = current_user(&state, &headers).await {
        return response.into_response();
    }
    let request = RedactionPreviewRequestBody(body.0.normalized());
    match redact::preview(&request.0) {
        Ok(preview) => Json(RedactionPreviewResponseBody { preview }).into_response(),
        Err(err) => bad_request(&err.to_string()),
    }
}

pub(super) async fn list_redaction_custom_strings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RedactionRulePageQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let (scope, user_id) = match resolve_redaction_scope(
        &user,
        RedactionSettingQuery {
            scope: query.scope,
            user_id: query.user_id,
        },
    ) {
        Ok(target) => target,
        Err(response) => return response.into_response(),
    };
    let (first, rows) = normalize_redaction_page(query.first, query.rows);
    match db::list_redaction_custom_string_rules(
        &state.pool,
        matches!(scope, RedactionScope::Global),
        user_id,
        first,
        rows,
        query.search.as_deref(),
    )
    .await
    {
        Ok((items, total)) => Json(RedactionCustomStringRulePageResponse {
            items: items
                .into_iter()
                .map(|item| RedactionCustomStringRuleRow {
                    array_index: item.array_index,
                    pattern: item.pattern,
                    match_type: item.match_type,
                    scope: item.scope,
                    created_at: item.created_at,
                    updated_at: item.updated_at,
                })
                .collect(),
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

fn resolve_redaction_scope(
    user: &SessionUser,
    query: RedactionSettingQuery,
) -> Result<(RedactionScope, Option<i64>), ApiError> {
    let scope = query.scope.unwrap_or(if user.is_admin {
        RedactionScope::Global
    } else {
        RedactionScope::User
    });
    match scope {
        RedactionScope::Global if !user.is_admin => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "admin required for global redaction rules",
        )),
        RedactionScope::Global => Ok((scope, None)),
        RedactionScope::User => {
            let target_user_id = query.user_id.unwrap_or(user.user_id);
            if !user.is_admin && target_user_id != user.user_id {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "cannot access another user's redaction rules",
                ));
            }
            Ok((scope, Some(target_user_id)))
        }
    }
}

fn normalize_redaction_page(first: Option<i64>, rows: Option<i64>) -> (i64, i64) {
    (first.unwrap_or(0).max(0), rows.unwrap_or(10).clamp(1, 100))
}

pub(super) async fn get_request_content_logging(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    Json(state.request_content_logging.read().await.clone()).into_response()
}

pub(super) async fn set_request_content_logging(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<RequestContentLoggingRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let normalized = RequestContentLoggingResponse {
        mode: body.mode,
        raw_retention_days: body.raw_retention_days,
    };
    if let Some(pool) = state.config_repository.as_postgres() {
        match db::set_request_content_logging(pool, &normalized).await {
            Ok(config) => {
                *state.request_content_logging.write().await = config.clone();
                if let Ok(retention) = db::get_usage_retention(pool).await {
                    *state.usage_retention.write().await = retention;
                }
                return Json(config).into_response();
            }
            Err(err) => return internal(&state, err),
        }
    }
    // SQLite: keep usage retention coherent in memory and persist the JSON.
    if let Err(err) = state
        .config_repository
        .set_json_setting("request_content_logging", &normalized)
        .await
    {
        return internal(&state, err);
    }
    if let Err(err) = state
        .config_repository
        .set_json_setting("usage_retention", &*state.usage_retention.read().await)
        .await
    {
        return internal(&state, err);
    }
    *state.request_content_logging.write().await = normalized.clone();
    Json(normalized).into_response()
}

pub(super) async fn get_usage_retention(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    Json(state.usage_retention.read().await.clone()).into_response()
}

pub(super) async fn set_usage_retention(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<UsageRetentionSettings>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Some(pool) = state.config_repository.as_postgres() {
        match db::set_usage_retention(pool, &body).await {
            Ok(config) => {
                *state.usage_retention.write().await = config.clone();
                state
                    .request_content_logging
                    .write()
                    .await
                    .raw_retention_days = config.raw_retention_days;
                return Json(config).into_response();
            }
            Err(err) => return internal(&state, err),
        }
    }
    if let Err(err) = state
        .config_repository
        .set_json_setting("usage_retention", &body)
        .await
    {
        return internal(&state, err);
    }
    *state.usage_retention.write().await = body.clone();
    Json(body).into_response()
}

fn normalize_stream_delta_batching_settings(
    mut value: db::StreamDeltaBatchingSettings,
) -> db::StreamDeltaBatchingSettings {
    value.flush_window_ms = value.flush_window_ms.clamp(1, 1_000);
    value.max_buffer_chars = value.max_buffer_chars.clamp(1, 8_192);
    value.max_buffer_bytes = value.max_buffer_bytes.clamp(1, 65_536);
    value
}

pub(super) async fn get_stream_delta_batching(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    Json(state.stream_delta_batching.read().await.clone()).into_response()
}

pub(super) async fn set_stream_delta_batching(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<db::StreamDeltaBatchingSettings>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let normalized = normalize_stream_delta_batching_settings(body);
    match state
        .config_repository
        .set_json_setting("stream_delta_batching", &normalized)
        .await
    {
        Ok(()) => {
            *state.stream_delta_batching.write().await = normalized.clone();
            Json(normalized).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn get_endpoint_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    if let Some(pool) = state.config_repository.as_postgres() {
        match db::get_user_endpoint_setting(pool, user.user_id).await {
            Ok(endpoint_id) => {
                return Json(serde_json::json!({ "endpoint_id": endpoint_id })).into_response();
            }
            Err(err) => return internal(&state, err),
        }
    }
    // SQLite: pull from the standalone `user_endpoint_setting` table via the
    // repository layer (Phase 3 surface).
    match state
        .config_repository
        .get_user_endpoint_setting(user.user_id)
        .await
    {
        Ok(endpoint_id) => Json(serde_json::json!({ "endpoint_id": endpoint_id })).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn set_endpoint_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<EndpointSettingRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    if let Some(pool) = state.config_repository.as_postgres() {
        match db::set_user_endpoint_setting(pool, user.user_id, body.endpoint_id).await {
            Ok(()) => {
                let _ = publish_snapshot(&state).await;
                return StatusCode::NO_CONTENT.into_response();
            }
            Err(err) => return internal(&state, err),
        }
    }
    match state
        .config_repository
        .set_user_endpoint_setting(user.user_id, body.endpoint_id)
        .await
    {
        Ok(()) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn get_relay_ip_whitelist(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match state
        .config_repository
        .get_json_setting::<RelayIpPolicy>("relay_ip_whitelist")
        .await
    {
        Ok(policy) => Json(RelayIpPolicyResponse::from(policy.unwrap_or_default())).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn set_relay_ip_whitelist(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<RelayIpPolicy>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let policy = match validate_relay_ip_policy(body) {
        Ok(policy) => policy,
        Err(response) => return response.into_response(),
    };
    if let Err(err) = state
        .config_repository
        .set_json_setting("relay_ip_whitelist", &policy)
        .await
    {
        return internal(&state, err);
    }
    // The access policy is committed. A failed publication would leave the
    // relays running with the previous policy, so it is reported instead of
    // being ignored.
    if let Err(err) = publish_snapshot(&state).await {
        return internal(&state, err);
    }
    Json(RelayIpPolicyResponse::from(policy)).into_response()
}

pub(super) async fn get_model_route_whitelist(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match state
        .config_repository
        .get_bool_setting("model_route_whitelist_enabled", true)
        .await
    {
        Ok(enabled) => Json(ModelRouteWhitelistResponse { enabled }).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn set_model_route_whitelist(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ModelRouteWhitelistRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Err(err) = state
        .config_repository
        .set_bool_setting("model_route_whitelist_enabled", body.enabled)
        .await
    {
        return internal(&state, err);
    }
    state
        .model_route_whitelist_enabled
        .store(body.enabled, Ordering::SeqCst);
    Json(ModelRouteWhitelistResponse {
        enabled: body.enabled,
    })
    .into_response()
}

async fn shared_valkey_response(
    state: &AdminState,
    configured: bool,
    url: Option<&str>,
    ttl_seconds: u64,
    desired_revision: Option<i64>,
) -> Response {
    let applied_revision = state.shared_valkey.revision();
    let restart_required =
        desired_revision.is_some_and(|desired| Some(desired) != applied_revision);
    let workers_restart_required =
        match db::count_workers_requiring_restart(&state.pool, desired_revision).await {
            Ok(count) => count,
            Err(err) => return internal(state, err),
        };
    Json(SharedValkeySettingsResponse {
        configured,
        url_masked: url.map(db::mask_shared_valkey_url),
        ttl_seconds,
        desired_revision,
        applied_revision,
        restart_required,
        workers_restart_required,
    })
    .into_response()
}

pub(super) async fn get_shared_valkey(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if state.config_repository.is_sqlite() {
        return state.capability_unavailable(db::Capability::SharedValkey);
    }
    let Some(manager) = state.relay_secret_manager.as_ref() else {
        return internal(
            &state,
            anyhow::anyhow!("worker encryption key is unavailable"),
        );
    };
    let persisted = match db::get_shared_valkey_persisted(&state.pool).await {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };
    let config = match db::SharedValkeyConfig::from_persisted(persisted, manager) {
        Ok(config) => config,
        Err(err) => return internal(&state, err),
    };
    shared_valkey_response(
        &state,
        config.is_configured(),
        config.url(),
        config.ttl_seconds(),
        config.revision(),
    )
    .await
}

pub(super) async fn set_shared_valkey(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<SharedValkeySettingsRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if state.config_repository.is_sqlite() {
        return state.capability_unavailable(db::Capability::SharedValkey);
    }
    let Some(manager) = state.relay_secret_manager.as_ref() else {
        return internal(
            &state,
            anyhow::anyhow!("worker encryption key is unavailable"),
        );
    };
    let existing_persisted = match db::get_shared_valkey_persisted(&state.pool).await {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };
    let existing_config = if matches!(
        &body.url,
        Some(SharedValkeySecretPatch::Replace { .. } | SharedValkeySecretPatch::Clear)
    ) {
        None
    } else {
        match db::SharedValkeyConfig::from_persisted(existing_persisted.clone(), manager) {
            Ok(config) => Some(config),
            Err(err) => return internal(&state, err),
        }
    };
    if let Err(err) = db::validate_shared_valkey_ttl(body.ttl_seconds) {
        return bad_request(&err.to_string());
    }
    if let Some(SharedValkeySecretPatch::Replace { value }) = &body.url
        && let Err(err) = db::validate_shared_valkey_url(value.trim())
    {
        return bad_request(&err.to_string());
    }
    let url_plaintext = match &body.url {
        Some(SharedValkeySecretPatch::Clear) => None,
        Some(SharedValkeySecretPatch::Replace { value }) => Some(value.trim().to_string()),
        _ => existing_config
            .as_ref()
            .and_then(|config| config.url())
            .map(str::to_string),
    };
    // The write is serialized inside the database and carries the revision
    // this request read. A concurrent writer wins the lock first and this
    // request reports a conflict instead of overwriting the newer value.
    let expected_revision = existing_persisted
        .as_ref()
        .map(|persisted| persisted.revision);
    let outcome = match db::save_shared_valkey_settings(
        &state.pool,
        manager,
        url_plaintext.as_deref(),
        body.ttl_seconds,
        expected_revision,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(err) => return internal(&state, err),
    };
    let persisted = match outcome {
        db::SharedValkeyWriteOutcome::Written(persisted) => persisted,
        db::SharedValkeyWriteOutcome::Conflict { current_revision } => {
            return error(
                StatusCode::CONFLICT,
                "shared_valkey_conflict",
                &format!(
                    "the shared Valkey settings changed concurrently (current revision {:?}); \
                     reload and retry",
                    current_revision
                ),
            );
        }
    };
    shared_valkey_response(
        &state,
        persisted.url.is_some(),
        url_plaintext.as_deref(),
        persisted.ttl_seconds,
        Some(persisted.revision),
    )
    .await
}
