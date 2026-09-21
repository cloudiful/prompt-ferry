use super::*;

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
    Json(settings.redacted()).into_response()
}

pub(super) async fn set_cache_alert_setting(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CacheAlertSettings>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(pool) = state.config_repository.as_postgres() else {
        return state.sqlite_capability_unavailable();
    };
    if let Err(err) = body.validate() {
        return bad_request(&err.to_string());
    }
    // The DingTalk secret is write-only: a blank value keeps the stored one,
    // so a GET -> edit -> PUT round trip cannot wipe it.
    let stored = state.cache_alert.read().await.clone();
    let mut next = body.normalized();
    next.dingtalk_secret = if next.dingtalk_secret.trim().is_empty() {
        stored.dingtalk_secret
    } else {
        next.dingtalk_secret.trim().to_string()
    };
    match db::set_cache_alert_settings(pool, &next).await {
        Ok(saved) => {
            *state.cache_alert.write().await = saved.clone();
            Json(saved.redacted()).into_response()
        }
        Err(err) => internal(&state, err),
    }
}
