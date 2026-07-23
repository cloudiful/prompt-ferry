use super::*;
use super::{
    relay_input::{resolve_create_relay_input, resolve_update_relay_input},
    relay_validation::map_relay_db_error,
};

pub(super) async fn list_relays(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let relays = match db::list_managed_relays(&state.pool).await {
        Ok(relays) => relays,
        Err(err) => return internal(&state, err),
    };
    let runtime = state.managed_relay_statuses.read().await.clone();
    Json(ManagedRelayListResponse {
        relays: relays
            .into_iter()
            .map(|relay| {
                ManagedRelay::from_parts(
                    relay.clone(),
                    runtime.get(&relay.relay_id).cloned().unwrap_or_default(),
                )
            })
            .collect(),
    })
    .into_response()
}

pub(super) async fn get_relay(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(relay_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let relay = match db::get_managed_relay(&state.pool, relay_id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "relay not found"),
        Err(err) => return internal(&state, err),
    };
    Json(ManagedRelay::from_parts(
        relay,
        state.managed_runtime_status_or_default(relay_id).await,
    ))
    .into_response()
}

pub(super) async fn create_relay(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ManagedRelayRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if let Err(message) = body.validate_create() {
        return bad_request(&message);
    }
    let input = match resolve_create_relay_input(&state, body).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let relay = match db::create_managed_relay(&state.pool, input).await {
        Ok(relay) => relay,
        Err(err) => return map_relay_db_error(&state, err),
    };
    if let Err(err) = state.reconcile_relays().await {
        return internal(&state, err);
    }
    Json(ManagedRelay::from_parts(
        relay.clone(),
        state
            .managed_runtime_status_or_default(relay.relay_id)
            .await,
    ))
    .into_response()
}

pub(super) async fn update_relay(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(relay_id): Path<Uuid>,
    Json(body): Json<ManagedRelayPatchRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let existing = match db::get_managed_relay(&state.pool, relay_id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "relay not found"),
        Err(err) => return internal(&state, err),
    };
    if let Err(message) = body.validate_patch(existing.tls_mode()) {
        return bad_request(&message);
    }
    let input = match resolve_update_relay_input(&state, existing, body).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let relay = match db::update_managed_relay(&state.pool, relay_id, input).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "relay not found"),
        Err(err) => return map_relay_db_error(&state, err),
    };
    if let Err(err) = state.reconcile_relays().await {
        return internal(&state, err);
    }
    Json(ManagedRelay::from_parts(
        relay.clone(),
        state
            .managed_runtime_status_or_default(relay.relay_id)
            .await,
    ))
    .into_response()
}

pub(super) async fn delete_relay(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(relay_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::delete_managed_relay(&state.pool, relay_id).await {
        Ok(true) => {
            if let Err(err) = state.reconcile_relays().await {
                return internal(&state, err);
            }
            state.managed_relay_statuses.write().await.remove(&relay_id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "relay not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn reconnect_relay(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(relay_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let relay = match db::get_managed_relay(&state.pool, relay_id).await {
        Ok(Some(relay)) => relay,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "relay not found"),
        Err(err) => return internal(&state, err),
    };
    if !relay.enabled {
        return bad_request("relay is disabled");
    }
    if let Err(err) = state.reconnect_relay(relay_id).await {
        return internal(&state, err);
    }
    Json(ManagedRelay::from_parts(
        relay.clone(),
        state
            .managed_runtime_status_or_default(relay.relay_id)
            .await,
    ))
    .into_response()
}
