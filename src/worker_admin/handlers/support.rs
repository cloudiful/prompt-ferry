use super::*;

pub async fn publish_snapshot(state: &AdminState) -> anyhow::Result<i64> {
    let keys = db::snapshot_keys(&state.pool).await?;
    let relay_ip_policy = db::get_json_setting::<RelayIpPolicy>(&state.pool, "relay_ip_whitelist")
        .await?
        .unwrap_or_default();
    let version = state.snapshot_version.fetch_add(1, Ordering::SeqCst) + 1;
    let snapshot = ConfigSnapshot {
        version,
        keys: keys
            .into_iter()
            .map(|key| ClientRoute {
                key_hash: key.key_hash,
                key_prefix: key.key_prefix,
                user_id: key.user_id,
                route_id: key.route_id.to_string(),
            })
            .collect(),
        relay_ip_policy,
    };
    let mut relay_senders = state.relay_senders.lock().await;
    let relay_urls = relay_senders.keys().cloned().collect::<Vec<_>>();
    let mut disconnected = Vec::new();
    for relay_url in relay_urls {
        let Some(tx) = relay_senders.get(&relay_url) else {
            continue;
        };
        if tx
            .send(BridgeMessage::ConfigSnapshot(snapshot.clone()))
            .is_err()
        {
            disconnected.push(relay_url);
        } else if let Ok(relay_id) = relay_url.parse::<Uuid>()
            && let Some(status) = state
                .managed_relay_statuses
                .write()
                .await
                .get_mut(&relay_id)
        {
            status.last_snapshot_version = Some(version);
        }
    }
    for relay_url in disconnected {
        relay_senders.remove(&relay_url);
        if let Ok(relay_id) = relay_url.parse::<Uuid>()
            && let Some(status) = state
                .managed_relay_statuses
                .write()
                .await
                .get_mut(&relay_id)
        {
            status.connected = false;
            status.last_disconnected_at = Some(chrono::Utc::now());
        }
    }
    Ok(version)
}

pub async fn set_bridge_sender(
    state: &AdminState,
    relay_url: &str,
    sender: Option<mpsc::UnboundedSender<BridgeMessage>>,
) {
    let mut relay_senders = state.relay_senders.lock().await;
    match sender {
        Some(sender) => {
            relay_senders.insert(relay_url.to_string(), sender);
        }
        None => {
            relay_senders.remove(relay_url);
        }
    }
}

pub(super) async fn resolve_endpoint_input(
    state: &AdminState,
    body: EndpointRequest,
    existing_endpoint_api_keys: Option<Vec<db::EndpointApiKey>>,
) -> Result<EndpointCreate, Response> {
    validate_request_budget_limit(body.daily_max_requests, "daily_max_requests")
        .map_err(|response| *response)?;
    validate_request_budget_limit(body.monthly_max_requests, "monthly_max_requests")
        .map_err(|response| *response)?;
    if !matches!(body.scope.as_str(), "admin" | "user") {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "scope must be admin or user",
        ));
    }
    if body.scope == "admin" && body.owner_user_id.is_some() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_owner",
            "admin endpoint cannot have owner",
        ));
    }
    if body.scope == "user" && body.owner_user_id.is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_owner",
            "user endpoint requires owner",
        ));
    }
    if let Some(owner_user_id) = body.owner_user_id {
        let owner = db::get_active_user(&state.pool, owner_user_id)
            .await
            .map_err(|err| internal(state, err))?;
        if owner.is_none() {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "owner user not found or inactive",
            ));
        }
    }
    let (native_api, native_api_source) = match body.protocol_mode {
        EndpointProtocolMode::Manual => {
            let native_api = body.native_api_override.ok_or_else(|| {
                error(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "native_api_override is required in manual protocol mode",
                )
            })?;
            if native_api == NativeApi::Auto {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "auto must use automatic protocol mode",
                ));
            }
            (native_api, NativeApiSource::Manual)
        }
        EndpointProtocolMode::Auto => {
            if body.native_api_override.is_some() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "native_api_override is only valid in manual protocol mode",
                ));
            }
            (NativeApi::Auto, NativeApiSource::Auto)
        }
    };
    let existing_api_keys = existing_endpoint_api_keys.unwrap_or_default();
    let mut submitted_key_labels = std::collections::HashSet::<String>::new();
    let mut api_keys = Vec::with_capacity(body.api_keys.len());
    for (index, submitted) in body.api_keys.into_iter().enumerate() {
        let key_label = submitted.key_label.trim();
        let raw_api_key = submitted.api_key.trim();
        if !key_label.is_empty() && !submitted_key_labels.insert(key_label.to_string()) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "endpoint api key labels must not contain duplicates",
            ));
        }
        let existing_key = if let Some(key_id) = submitted.key_id {
            let matched = existing_api_keys.iter().find(|key| key.key_id == key_id);
            if matched.is_none() {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "invalid_endpoint_key",
                    "endpoint key not found for this endpoint",
                ));
            }
            matched
        } else if !key_label.is_empty() {
            existing_api_keys
                .iter()
                .find(|key| key.key_label == key_label)
        } else {
            None
        };
        let resolved_api_key = if raw_api_key.is_empty() {
            existing_key
                .map(|key| key.api_key.clone())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    error(
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        "endpoint api key value is required",
                    )
                })?
        } else {
            raw_api_key.to_string()
        };
        api_keys.push(db::EndpointApiKeyCreate {
            key_label: if key_label.is_empty() {
                existing_key
                    .map(|key| key.key_label.clone())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| format!("key {}", index + 1))
            } else {
                key_label.to_string()
            },
            api_key: resolved_api_key,
            position: i32::try_from(index).unwrap_or(i32::MAX),
            enabled: submitted
                .enabled
                .unwrap_or_else(|| existing_key.map(|key| key.enabled).unwrap_or(true)),
            key_id: existing_key.map(|key| key.key_id),
        });
    }
    if api_keys.is_empty() && !body.api_key.trim().is_empty() {
        api_keys.push(db::EndpointApiKeyCreate {
            key_label: if body.name.trim().is_empty() {
                "key 1".to_string()
            } else {
                body.name.trim().to_string()
            },
            api_key: body.api_key.clone(),
            position: 0,
            enabled: true,
            key_id: None,
        });
    }
    if api_keys.is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "at least one endpoint api key is required",
        ));
    }
    if !api_keys.iter().any(|key| key.enabled) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "at least one endpoint api key must be enabled",
        ));
    }
    let api_key = api_keys[0].api_key.clone();
    Ok(EndpointCreate {
        scope: body.scope,
        owner_user_id: body.owner_user_id,
        name: body.name,
        base_url: body.base_url,
        native_api,
        native_api_source,
        daily_max_requests: body.daily_max_requests,
        monthly_max_requests: body.monthly_max_requests,
        api_key,
        api_keys,
        key_lb_enabled: body.key_lb_enabled,
        enabled: body.enabled.unwrap_or(true),
    })
}

pub(super) fn endpoint_base_url_has_version_path(base_url: &str) -> bool {
    base_url.trim().trim_end_matches('/').ends_with("/v1")
}

pub(super) fn parse_native_api(value: &str) -> NativeApi {
    serde_json::from_value(Value::String(value.to_string())).unwrap_or(NativeApi::Responses)
}

pub(super) fn validate_request_budget_limit(
    value: Option<i32>,
    field_name: &str,
) -> Result<(), Box<Response>> {
    if value.is_some_and(|limit| limit <= 0) {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "invalid_budget_limit",
            &format!("{field_name} must be greater than 0"),
        )));
    }
    Ok(())
}

pub(super) fn validate_relay_ip_policy(
    policy: RelayIpPolicy,
) -> Result<RelayIpPolicy, Box<Response>> {
    let policy = ip_acl::normalize_policy(&policy);
    if let Err(err) = ip_acl::compile_policy(&policy) {
        return Err(Box::new(bad_request(&format!(
            "invalid relay IP whitelist: {err}"
        ))));
    }
    Ok(policy)
}

pub(super) fn truncate_message(message: &str) -> String {
    if message.is_empty() {
        return "empty response".to_string();
    }
    let truncated = message.chars().take(240).collect::<String>();
    if truncated.len() < message.len() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
