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
    Json(settings.redacted_response()).into_response()
}

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
    let stored = state.cache_alert.read().await.clone();
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
