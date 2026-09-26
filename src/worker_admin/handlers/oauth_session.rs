//! Issue #599 R2b: pending ChatGPT login registry and R2a token persistence.
//!
//! Pending logins are process-local by design (single self-hosted worker): a
//! restart only drops in-flight flows, while stored tokens live in the
//! encrypted envelope and survive it. The OAuth handlers share these helpers.

use std::sync::{LazyLock, Mutex, MutexGuard};

use reqwest::Client;

use super::chatgpt_backend::token_expiry;
use super::oauth_client::{CHATGPT_ISSUER, ChatgptOAuthError, OAuthTokenResponse};
use super::*;

#[derive(Clone)]
pub(super) struct PendingDeviceFlow {
    pub(super) endpoint_id: Uuid,
    pub(super) device_auth_id: String,
    pub(super) user_code: String,
    pub(super) expires_at: Instant,
}

#[derive(Clone)]
pub(super) struct PendingBrowserFlow {
    pub(super) endpoint_id: Uuid,
    pub(super) code_verifier: String,
    pub(super) redirect_uri: String,
    pub(super) oauth_state: String,
    pub(super) expires_at: Instant,
}

static DEVICE_FLOWS: LazyLock<Mutex<HashMap<Uuid, PendingDeviceFlow>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_FLOWS: LazyLock<Mutex<HashMap<Uuid, PendingBrowserFlow>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_flows<T>(flows: &Mutex<HashMap<Uuid, T>>) -> MutexGuard<'_, HashMap<Uuid, T>> {
    flows
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn store_device_flow(flow_id: Uuid, flow: PendingDeviceFlow) {
    let now = Instant::now();
    let mut flows = lock_flows(&DEVICE_FLOWS);
    // One live flow per endpoint; expired ones never accumulate.
    flows.retain(|_, existing| {
        existing.expires_at > now && existing.endpoint_id != flow.endpoint_id
    });
    flows.insert(flow_id, flow);
}

pub(super) fn device_flow(endpoint_id: Uuid, flow_id: Uuid) -> Option<PendingDeviceFlow> {
    lock_flows(&DEVICE_FLOWS)
        .get(&flow_id)
        .filter(|flow| flow.endpoint_id == endpoint_id)
        .cloned()
}

pub(super) fn remove_device_flow(flow_id: Uuid) {
    lock_flows(&DEVICE_FLOWS).remove(&flow_id);
}

pub(super) fn store_browser_flow(flow_id: Uuid, flow: PendingBrowserFlow) {
    let now = Instant::now();
    let mut flows = lock_flows(&BROWSER_FLOWS);
    flows.retain(|_, existing| {
        existing.expires_at > now && existing.endpoint_id != flow.endpoint_id
    });
    flows.insert(flow_id, flow);
}

pub(super) fn browser_flow(endpoint_id: Uuid, flow_id: Uuid) -> Option<PendingBrowserFlow> {
    lock_flows(&BROWSER_FLOWS)
        .get(&flow_id)
        .filter(|flow| flow.endpoint_id == endpoint_id)
        .cloned()
}

pub(super) fn remove_browser_flow(flow_id: Uuid) {
    lock_flows(&BROWSER_FLOWS).remove(&flow_id);
}

pub(super) async fn ensure_login_endpoint(
    state: &AdminState,
    endpoint_id: Uuid,
) -> Result<(), Response> {
    match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) if endpoint.provider.supports_chatgpt_subscription_plan() => Ok(()),
        Ok(Some(_)) => Err(error(
            StatusCode::BAD_REQUEST,
            "oauth_unsupported_provider",
            "ChatGPT OAuth login requires an openai endpoint",
        )),
        Ok(None) => Err(endpoint_not_found()),
        Err(err) => Err(internal(state, err)),
    }
}

/// Endpoint-aware protocol client: honors the endpoint proxy and fails closed.
pub(super) async fn oauth_client(
    state: &AdminState,
    endpoint_id: Uuid,
) -> Result<Client, Response> {
    let proxy_url = state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
        .map_err(|err| internal(state, err))?;
    crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(
        proxy_url.as_deref(),
        CHATGPT_ISSUER,
    )
    .map_err(|message| {
        error(
            StatusCode::BAD_REQUEST,
            "invalid_proxy_url",
            &truncate_message(&maybe_redact(state, &message)),
        )
    })
}

async fn stored_token_status(
    state: &AdminState,
    endpoint_id: Uuid,
) -> anyhow::Result<(bool, Option<chrono::DateTime<Utc>>)> {
    let Some(token) = state
        .config_repository
        .get_endpoint_oauth_token(endpoint_id)
        .await?
    else {
        return Ok((false, None));
    };
    Ok((true, token.expires_at))
}

pub(super) fn token_expired(expires_at: Option<chrono::DateTime<Utc>>) -> bool {
    expires_at.is_some_and(|expires_at| expires_at <= Utc::now())
}

pub(super) async fn oauth_status_response(state: &AdminState, endpoint_id: Uuid) -> Response {
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return endpoint_not_found(),
        Err(err) => return internal(state, err),
    };
    match stored_token_status(state, endpoint_id).await {
        Ok((has_oauth_token, expires_at)) => Json(EndpointOAuthStatusResponse {
            endpoint_id,
            plan: endpoint.plan,
            has_oauth_token,
            expires_at,
            expired: token_expired(expires_at),
        })
        .into_response(),
        Err(err) => internal(state, err),
    }
}

pub(super) async fn persist_token(
    state: &AdminState,
    endpoint_id: Uuid,
    access_token: String,
    refresh_token: String,
    expires_in_seconds: Option<u64>,
) -> Result<(), Response> {
    let token = db::EndpointOAuthTokenSet {
        access_token,
        refresh_token,
        expires_at: Some(token_expiry(expires_in_seconds)),
    };
    state
        .config_repository
        .set_endpoint_oauth_token(endpoint_id, Some(token))
        .await
        .map_err(|err| internal(state, err))?;
    publish_token_snapshot(state).await;
    Ok(())
}

pub(super) async fn publish_token_snapshot(state: &AdminState) {
    if let Err(err) = publish_snapshot(state).await {
        tracing::warn!(error = %err, "snapshot publication failed after ChatGPT OAuth token change");
    }
}

pub(super) async fn complete_login(
    state: &AdminState,
    endpoint_id: Uuid,
    flow_id: Uuid,
    tokens: OAuthTokenResponse,
) -> Response {
    let Some(refresh_token) = tokens.refresh_token else {
        return oauth_error_response(state, missing_refresh_token());
    };
    if let Err(response) = persist_token(
        state,
        endpoint_id,
        tokens.access_token,
        refresh_token,
        tokens.expires_in_seconds,
    )
    .await
    {
        return response;
    }
    match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => Json(OAuthLoginResponse {
            flow_id,
            status: OAuthLoginStatus::Complete,
            endpoint: Some(endpoint.into_pg()),
        })
        .into_response(),
        Ok(None) => endpoint_not_found(),
        Err(err) => internal(state, err),
    }
}

fn missing_refresh_token() -> ChatgptOAuthError {
    ChatgptOAuthError::Upstream {
        status: None,
        message: "ChatGPT OAuth response is missing refresh_token".to_string(),
    }
}

pub(super) fn oauth_error_response(state: &AdminState, oauth_error: ChatgptOAuthError) -> Response {
    let message = truncate_message(&maybe_redact(state, &oauth_error.to_string()));
    match oauth_error {
        ChatgptOAuthError::RateLimited(_) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "oauth_rate_limited",
            &message,
        ),
        ChatgptOAuthError::InvalidGrant(_) => {
            error(StatusCode::BAD_REQUEST, "oauth_invalid_grant", &message)
        }
        ChatgptOAuthError::AuthorizationDenied(_) => error(
            StatusCode::BAD_REQUEST,
            "oauth_authorization_denied",
            &message,
        ),
        ChatgptOAuthError::InvalidRedirect(_) => {
            error(StatusCode::BAD_REQUEST, "oauth_invalid_redirect", &message)
        }
        ChatgptOAuthError::Upstream { .. } => {
            error(StatusCode::BAD_GATEWAY, "oauth_upstream_error", &message)
        }
    }
}

pub(super) fn endpoint_not_found() -> Response {
    error(StatusCode::NOT_FOUND, "not_found", "endpoint not found")
}

pub(super) fn flow_not_found(message: &str) -> Response {
    error(StatusCode::NOT_FOUND, "oauth_flow_not_found", message)
}
