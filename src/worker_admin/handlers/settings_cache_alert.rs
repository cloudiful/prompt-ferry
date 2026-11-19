use super::*;

#[utoipa::path(
    get,
    path = "/api/v1/settings/cache-alert",
    responses((status = 200, body = CacheAlertSettingsResponse, description = "Continuous-session cache alert settings; the stored DingTalk secret and SMTP password are never echoed back and `has_dingtalk_secret` / `smtp.has_smtp_password` report whether one is stored")),
    tag = "settings"
)]
pub(super) async fn get_cache_alert_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if state.config_repository.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let settings = state.cache_alert.read().await.clone();
    Json(settings.redacted_response()).into_response()
}

#[utoipa::path(
    put,
    path = "/api/v1/settings/cache-alert",
    request_body = CacheAlertSettingsRequest,
    responses((status = 200, body = CacheAlertSettingsResponse, description = "Updated cache alert settings; a blank dingtalk_secret or smtp.password keeps the stored value and an omitted smtp object keeps the stored SMTP configuration")),
    tag = "settings"
)]
pub(super) async fn set_cache_alert_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CacheAlertSettingsRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(pool) = state.config_repository.as_postgres() else {
        return state.sqlite_capability_unavailable();
    };
    // The stored policy supplies anything the request deliberately leaves
    // blank: the write-only DingTalk secret, the SMTP password, and the whole
    // SMTP object when an older client omits it. Validation runs on the merged
    // policy so a kept password still satisfies the credential pair.
    //
    // The stored policy is read from the database, not from the in-memory
    // state: another worker may have written a newer policy and a stale copy
    // would silently overwrite its credentials.
    let stored = match db::get_cache_alert_settings(pool).await {
        Ok(stored) => stored,
        Err(err) => return internal(&state, err),
    };
    let next = body.into_settings(&stored);
    if let Err(err) = next.validate() {
        return bad_request(&err.to_string());
    }
    match db::set_cache_alert_settings(pool, &next).await {
        Ok(saved) => {
            *state.cache_alert.write().await = saved.clone();
            Json(saved.redacted_response()).into_response()
        }
        Err(err) => internal(&state, err),
    }
}
