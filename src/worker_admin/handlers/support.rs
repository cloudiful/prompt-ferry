use super::*;
use crate::db::config_repository::relays as relay_repo;

pub async fn publish_snapshot(state: &AdminState) -> anyhow::Result<i64> {
    let snapshot = relay_repo::build_unified_snapshot(&state.config_repository, 0).await?;
    let version = state.snapshot_version.fetch_add(1, Ordering::SeqCst) + 1;
    let config_snapshot = ConfigSnapshot {
        version,
        keys: snapshot.keys.clone(),
        relay_ip_policy: snapshot.relay_ip_policy,
    };
    let mut relay_senders = state.relay_senders.lock().await;
    let relay_urls = relay_senders.keys().cloned().collect::<Vec<_>>();
    let mut disconnected = Vec::new();
    for relay_url in relay_urls {
        let Some(tx) = relay_senders.get(&relay_url) else {
            continue;
        };
        if tx
            .send(BridgeMessage::ConfigSnapshot(config_snapshot.clone()))
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
    existing_proxy_url: Option<String>,
    existing_active_windows: Option<Vec<db::ActiveWindow>>,
) -> Result<EndpointCreate, ApiError> {
    validate_mcp_provider(body.mcp_enabled, body.provider).map_err(|message| {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_mcp_provider", message)
    })?;
    if !matches!(body.scope.as_str(), "admin" | "user") {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "scope must be admin or user",
        ));
    }
    if body.scope == "admin" && body.owner_user_id.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_owner",
            "admin endpoint cannot have owner",
        ));
    }
    if body.scope == "user" && body.owner_user_id.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_owner",
            "user endpoint requires owner",
        ));
    }
    match (body.provider, body.provider_region) {
        (db::EndpointProvider::Generic, Some(_))
        | (db::EndpointProvider::CommandCode, Some(_))
        | (db::EndpointProvider::OpencodeGo, Some(_))
        | (db::EndpointProvider::OpenRouter, Some(_))
        | (db::EndpointProvider::Glm, Some(_))
        | (db::EndpointProvider::DeepSeek, Some(_))
        | (db::EndpointProvider::OpenAi, Some(_)) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_provider_region",
                "provider_region is only valid for MiniMax endpoints",
            ));
        }
        (db::EndpointProvider::Minimax, None) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_provider_region",
                "MiniMax endpoints require provider_region",
            ));
        }
        _ => {}
    }
    if let Some(owner_user_id) = body.owner_user_id {
        let owner = db::get_active_user(&state.pool, owner_user_id)
            .await
            .map_err(|err| ApiError::internal(state, err))?;
        if owner.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_owner",
                "owner user not found or inactive",
            ));
        }
    }
    let (native_api, native_api_source) = match body.protocol_mode {
        EndpointProtocolMode::Manual => {
            let native_api = body.native_api_override.ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "native_api_override is required in manual protocol mode",
                )
            })?;
            if native_api == NativeApi::Auto {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "auto must use automatic protocol mode",
                ));
            }
            (native_api, NativeApiSource::Manual)
        }
        EndpointProtocolMode::Auto => {
            if body.native_api_override.is_some() {
                return Err(ApiError::new(
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
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "endpoint api key labels must not contain duplicates",
            ));
        }
        let existing_key = if let Some(key_id) = submitted.key_id {
            let matched = existing_api_keys.iter().find(|key| key.key_id == key_id);
            if matched.is_none() {
                return Err(ApiError::new(
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
                    ApiError::new(
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
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "at least one endpoint api key is required",
        ));
    }
    if !api_keys.iter().any(|key| key.enabled) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "at least one endpoint api key must be enabled",
        ));
    }
    let api_key = api_keys[0].api_key.clone();
    // Issue #368 Phase B: outbound proxy default. `None` (omitted/null)
    // carries the stored value on PATCH (`None` on create means direct);
    // empty/whitespace clears to direct; non-empty must pass the scheme
    // whitelist. `has_proxy_url` is accepted for forward-compat and ignored.
    let proxy_url = match body.proxy_url.as_deref() {
        None => existing_proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        Some(raw) if raw.trim().is_empty() => None,
        Some(raw) => Some(validate_proxy_url(raw.trim()).map_err(|message| {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_proxy_url", message)
        })?),
    };
    // Issue #392 Phase K: endpoint default windows reuse the same HH:MM
    // validation as targets. `None` keeps the stored value on PATCH
    // (`None` on create means all-day); `Some([])` means all-day.
    let active_windows = match body.active_windows {
        None => existing_active_windows,
        Some(windows) => Some(db::normalize_request_windows(&windows).map_err(|message| {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_active_windows", message)
        })?),
    };
    // Issue #248: preset providers ignore the client-sent base entirely and
    // persist the derived official root; Generic keeps the normalized
    // client-sent base byte-for-byte.
    let base_url = match crate::upstream_presets::preset_base_url(
        body.provider.into(),
        body.provider_region.map(|region| region.into()),
        native_api.into(),
    ) {
        Some(derived) => derived.to_string(),
        None => normalize_endpoint_base_url(&body.base_url),
    };
    Ok(EndpointCreate {
        scope: body.scope,
        owner_user_id: body.owner_user_id,
        name: body.name,
        provider: body.provider,
        provider_region: body.provider_region,
        service_tier: body.service_tier,
        base_url,
        native_api,
        native_api_source,
        api_key,
        api_keys,
        key_lb_enabled: body.key_lb_enabled,
        enabled: body.enabled.unwrap_or(true),
        proxy_url,
        active_windows,
    })
}

/// Issue #368 Phase B: normalize and validate an outbound proxy URL.
/// Returns the trimmed URL on success. Never echoes userinfo in errors.
pub(super) fn validate_proxy_url(trimmed: &str) -> Result<String, &'static str> {
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "proxy_url scheme must be one of http, https, socks5, socks5h")?;
    match parsed.scheme().to_ascii_lowercase().as_str() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => return Err("proxy_url scheme must be one of http, https, socks5, socks5h"),
    }
    if parsed.host_str().is_none_or(|host| host.trim().is_empty()) {
        return Err("proxy_url must include a host");
    }
    Ok(trimmed.to_string())
}

/// Normalize a Generic endpoint `base_url` for persistence.
///
/// Strips trailing `/v1` segments (and a chained `…/v1/v1`) so the stored
/// value matches the canonical API root that the upstream URL composer
/// uses. Preset providers no longer call this: their base is derived
/// server-side (issue #248), so the GLM exemption from issue #241 is gone.
pub(super) fn normalize_endpoint_base_url(base_url: &str) -> String {
    let mut v = base_url.trim().to_string();
    loop {
        v = v.trim_end_matches('/').to_string();
        match v.strip_suffix("/v1") {
            Some(s) => v = s.to_string(),
            None => break,
        }
    }
    v
}

pub(super) fn validate_mcp_provider(
    mcp_enabled: Option<bool>,
    provider: db::EndpointProvider,
) -> std::result::Result<(), &'static str> {
    // MCP exposure is only valid for MiniMax endpoints; an explicit true on a
    // non-MiniMax provider must be rejected. None and Some(false) are
    // accepted for any provider (the caller will collapse them to false for
    // non-MiniMax endpoints when persisting). CommandCode, OpencodeGo and
    // OpenRouter follow the generic path: they may be created with
    // mcp_enabled=false but never gain the MiniMax builtin MCP privilege.
    if mcp_enabled.unwrap_or(provider == db::EndpointProvider::Minimax)
        && provider != db::EndpointProvider::Minimax
    {
        return Err("MCP exposure requires a MiniMax endpoint");
    }
    Ok(())
}

pub(super) fn validate_relay_ip_policy(policy: RelayIpPolicy) -> Result<RelayIpPolicy, ApiError> {
    let policy = ip_acl::normalize_policy(&policy);
    if let Err(err) = ip_acl::compile_policy(&policy) {
        return Err(ApiError::bad_request(format!(
            "invalid relay IP whitelist: {err}"
        )));
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

#[cfg(test)]
mod tests {
    use super::{normalize_endpoint_base_url, validate_mcp_provider};
    use crate::db::EndpointProvider;

    #[test]
    fn normalize_endpoint_base_url_strips_trailing_v1_chain() {
        for (input, expected) in [
            ("https://api.openai.com", "https://api.openai.com"),
            ("https://api.openai.com/", "https://api.openai.com"),
            ("https://api.openai.com/v1", "https://api.openai.com"),
            ("https://api.openai.com/v1/", "https://api.openai.com"),
            ("https://api.openai.com/v1/v1", "https://api.openai.com"),
            ("https://api.openai.com/v1/v1/", "https://api.openai.com"),
            ("  https://api.openai.com/v1  ", "https://api.openai.com"),
            (
                "https://api.commandcode.ai/provider/v1",
                "https://api.commandcode.ai/provider",
            ),
            ("https://openrouter.ai/api/v1", "https://openrouter.ai/api"),
            (
                "https://api.commandcode.ai/provider",
                "https://api.commandcode.ai/provider",
            ),
            ("https://api.openai.com/V1", "https://api.openai.com/V1"),
            ("https://api.openai.com/v10", "https://api.openai.com/v10"),
        ] {
            assert_eq!(
                normalize_endpoint_base_url(input),
                expected,
                "input {input:?}"
            );
        }
    }

    #[test]
    fn validate_mcp_provider_accepts_explicit_false_for_generic() {
        assert!(validate_mcp_provider(Some(false), EndpointProvider::Generic).is_ok());
    }

    #[test]
    fn validate_mcp_provider_accepts_none_for_generic() {
        assert!(validate_mcp_provider(None, EndpointProvider::Generic).is_ok());
    }

    #[test]
    fn validate_mcp_provider_rejects_explicit_true_for_generic() {
        assert!(validate_mcp_provider(Some(true), EndpointProvider::Generic).is_err());
    }

    #[test]
    fn validate_mcp_provider_accepts_any_value_for_minimax() {
        assert!(validate_mcp_provider(None, EndpointProvider::Minimax).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::Minimax).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::Minimax).is_ok());
    }

    #[test]
    fn validate_mcp_provider_treats_command_code_like_generic() {
        assert!(validate_mcp_provider(None, EndpointProvider::CommandCode).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::CommandCode).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::CommandCode).is_err());
    }

    #[test]
    fn validate_mcp_provider_treats_opencode_go_like_generic() {
        assert!(validate_mcp_provider(None, EndpointProvider::OpencodeGo).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::OpencodeGo).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::OpencodeGo).is_err());
    }

    #[test]
    fn validate_mcp_provider_treats_openrouter_like_generic() {
        assert!(validate_mcp_provider(None, EndpointProvider::OpenRouter).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::OpenRouter).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::OpenRouter).is_err());
    }

    #[test]
    fn validate_mcp_provider_treats_glm_like_generic() {
        // GLM (issue #230 P1) follows the generic path: mcp_enabled must
        // collapse to false because GLM never gains the MiniMax builtin
        // MCP privilege. The DB CHECKs (0076) and the 401/400 handlers
        // already reject a true value for non-MiniMax providers; this test
        // pins the contract for the GLM variant specifically.
        assert!(validate_mcp_provider(None, EndpointProvider::Glm).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::Glm).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::Glm).is_err());
    }

    #[test]
    fn validate_mcp_provider_treats_openai_like_generic() {
        // OpenAI (issue #589 P1) follows the generic path: it never gains the
        // MiniMax builtin MCP privilege and must carry no provider region.
        assert!(validate_mcp_provider(None, EndpointProvider::OpenAi).is_ok());
        assert!(validate_mcp_provider(Some(false), EndpointProvider::OpenAi).is_ok());
        assert!(validate_mcp_provider(Some(true), EndpointProvider::OpenAi).is_err());
    }
}
