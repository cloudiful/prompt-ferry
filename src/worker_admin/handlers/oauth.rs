//! Issue #599 R2b: per-endpoint ChatGPT (Codex) OAuth login routes, token
//! refresh, and clear.
//!
//! The protocol client lives in the private `oauth_client` module and the
//! pending-login registry plus R2a persistence in `oauth_session`; this module
//! owns the admin routes and is the public test surface (`worker_admin::oauth`).

use super::oauth_client::BROWSER_REDIRECT_URI;
use super::oauth_session::{
    PendingBrowserFlow, PendingDeviceFlow, browser_flow, complete_login, device_flow,
    endpoint_not_found, ensure_login_endpoint, flow_not_found, oauth_client, oauth_error_response,
    oauth_status_response, persist_token, publish_token_snapshot, remove_browser_flow,
    remove_device_flow, store_browser_flow, store_device_flow, token_expired,
};
use super::*;

pub use super::oauth_client::{
    ChatgptOAuthError, DeviceAuthorization, DevicePollOutcome, OAuthTokenResponse, PkceCodes,
    authorize_url, exchange_authorization_code, generate_pkce, parse_redirect_url,
    poll_device_authorization, random_state, refresh_chatgpt_tokens, request_device_authorization,
};

const DEVICE_FLOW_TTL: Duration = Duration::from_secs(15 * 60);
const BROWSER_FLOW_TTL: Duration = Duration::from_secs(10 * 60);
const DEVICE_POLL_SAFETY_MARGIN_SECONDS: u64 = 3;
const DEVICE_FLOW_EXPIRED: &str = "the ChatGPT device login expired; start a new login";
const BROWSER_FLOW_EXPIRED: &str = "the ChatGPT browser login expired; start a new login";
const DEVICE_FLOW_MISSING: &str = "no pending ChatGPT device login for this endpoint";
const BROWSER_FLOW_MISSING: &str = "no pending ChatGPT browser login for this endpoint";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route(
            "/admin/endpoints/{endpoint_id}/oauth",
            get(oauth_status).delete(oauth_clear),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/oauth/device",
            post(oauth_device_start),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/oauth/device/poll",
            post(oauth_device_poll),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/oauth/browser",
            post(oauth_browser_start),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/oauth/browser/complete",
            post(oauth_browser_complete),
        )
        .route(
            "/admin/endpoints/{endpoint_id}/oauth/refresh",
            post(oauth_refresh),
        )
}

/// ChatGPT OAuth issuer. The deployment override lets integration tests point
/// the client at a mock issuer and lets air-gapped installs front it.
fn oauth_issuer() -> String {
    std::env::var("PROMPT_FERRY_CHATGPT_OAUTH_ISSUER")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| super::oauth_client::CHATGPT_ISSUER.to_string())
}

pub(super) async fn oauth_status(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    oauth_status_response(&state, endpoint_id).await
}

pub(super) async fn oauth_device_start(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Err(response) = ensure_login_endpoint(&state, endpoint_id).await {
        return response;
    }
    let client = match oauth_client(&state, endpoint_id).await {
        Ok(client) => client,
        Err(response) => return response,
    };
    let issuer = oauth_issuer();
    let device = match request_device_authorization(&client, &issuer).await {
        Ok(device) => device,
        Err(error) => return oauth_error_response(&state, error),
    };
    let flow_id = Uuid::new_v4();
    store_device_flow(
        flow_id,
        PendingDeviceFlow {
            endpoint_id,
            device_auth_id: device.device_auth_id,
            user_code: device.user_code.clone(),
            expires_at: Instant::now() + DEVICE_FLOW_TTL,
        },
    );
    Json(OAuthDeviceStartResponse {
        flow_id,
        user_code: device.user_code,
        verification_uri: format!("{issuer}/codex/device"),
        // Poll after the upstream interval plus a safety margin (matches the
        // Codex client) so a slow confirmation is never polled twice.
        interval_seconds: device.interval_seconds + DEVICE_POLL_SAFETY_MARGIN_SECONDS,
        expires_in_seconds: DEVICE_FLOW_TTL.as_secs(),
    })
    .into_response()
}

pub(super) async fn oauth_device_poll(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Json(body): Json<OAuthFlowRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(flow) = device_flow(endpoint_id, body.flow_id) else {
        return flow_not_found(DEVICE_FLOW_MISSING);
    };
    if flow.expires_at <= Instant::now() {
        remove_device_flow(body.flow_id);
        return error(StatusCode::GONE, "oauth_flow_expired", DEVICE_FLOW_EXPIRED);
    }
    let client = match oauth_client(&state, endpoint_id).await {
        Ok(client) => client,
        Err(response) => return response,
    };
    let issuer = oauth_issuer();
    let device = DeviceAuthorization {
        device_auth_id: flow.device_auth_id.clone(),
        user_code: flow.user_code.clone(),
        interval_seconds: 0,
    };
    let outcome = match poll_device_authorization(&client, &issuer, &device).await {
        Ok(outcome) => outcome,
        // Transient upstream failure: keep the flow so the poll can retry.
        Err(error) => return oauth_error_response(&state, error),
    };
    match outcome {
        DevicePollOutcome::Pending => Json(OAuthLoginResponse {
            flow_id: body.flow_id,
            status: OAuthLoginStatus::Pending,
            endpoint: None,
        })
        .into_response(),
        DevicePollOutcome::Denied(message) => {
            remove_device_flow(body.flow_id);
            error(
                StatusCode::BAD_REQUEST,
                "oauth_authorization_denied",
                &truncate_message(&maybe_redact(&state, &message)),
            )
        }
        DevicePollOutcome::Authorized {
            authorization_code,
            code_verifier,
        } => {
            remove_device_flow(body.flow_id);
            match exchange_authorization_code(
                &client,
                &issuer,
                &authorization_code,
                &format!("{issuer}/deviceauth/callback"),
                &code_verifier,
            )
            .await
            {
                Ok(tokens) => complete_login(&state, endpoint_id, body.flow_id, tokens).await,
                Err(error) => oauth_error_response(&state, error),
            }
        }
    }
}

pub(super) async fn oauth_browser_start(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Err(response) = ensure_login_endpoint(&state, endpoint_id).await {
        return response;
    }
    let pkce = generate_pkce();
    let oauth_state = random_state();
    let flow_id = Uuid::new_v4();
    store_browser_flow(
        flow_id,
        PendingBrowserFlow {
            endpoint_id,
            code_verifier: pkce.verifier.clone(),
            redirect_uri: BROWSER_REDIRECT_URI.to_string(),
            oauth_state: oauth_state.clone(),
            expires_at: Instant::now() + BROWSER_FLOW_TTL,
        },
    );
    let issuer = oauth_issuer();
    Json(OAuthBrowserStartResponse {
        flow_id,
        authorize_url: authorize_url(&issuer, BROWSER_REDIRECT_URI, &pkce, &oauth_state),
        redirect_uri: BROWSER_REDIRECT_URI.to_string(),
        expires_in_seconds: BROWSER_FLOW_TTL.as_secs(),
    })
    .into_response()
}

pub(super) async fn oauth_browser_complete(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Json(body): Json<OAuthBrowserCompleteRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let Some(flow) = browser_flow(endpoint_id, body.flow_id) else {
        return flow_not_found(BROWSER_FLOW_MISSING);
    };
    if flow.expires_at <= Instant::now() {
        remove_browser_flow(body.flow_id);
        return error(StatusCode::GONE, "oauth_flow_expired", BROWSER_FLOW_EXPIRED);
    }
    let code = match parse_redirect_url(&body.redirect_url, &flow.oauth_state) {
        Ok(code) => code,
        Err(error) => {
            if error.is_terminal_flow_error() {
                remove_browser_flow(body.flow_id);
            }
            return oauth_error_response(&state, error);
        }
    };
    let client = match oauth_client(&state, endpoint_id).await {
        Ok(client) => client,
        Err(response) => return response,
    };
    let exchange = exchange_authorization_code(
        &client,
        &oauth_issuer(),
        &code,
        &flow.redirect_uri,
        &flow.code_verifier,
    )
    .await;
    match exchange {
        Ok(tokens) => {
            remove_browser_flow(body.flow_id);
            complete_login(&state, endpoint_id, body.flow_id, tokens).await
        }
        Err(error) => {
            if error.is_terminal_flow_error() {
                remove_browser_flow(body.flow_id);
            }
            oauth_error_response(&state, error)
        }
    }
}

pub(super) async fn oauth_refresh(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    if let Err(response) = ensure_login_endpoint(&state, endpoint_id).await {
        return response;
    }
    let token = match state
        .config_repository
        .get_endpoint_oauth_token(endpoint_id)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "oauth_not_configured",
                "no ChatGPT OAuth token is stored for this endpoint",
            );
        }
        Err(err) => return internal(&state, err),
    };
    // Refresh on expiry only; a valid token is reported as-is.
    if !token_expired(token.expires_at) {
        return oauth_status_response(&state, endpoint_id).await;
    }
    let client = match oauth_client(&state, endpoint_id).await {
        Ok(client) => client,
        Err(response) => return response,
    };
    match refresh_chatgpt_tokens(&client, &oauth_issuer(), &token.refresh_token).await {
        Ok(tokens) => {
            // The refresh response may omit a rotated refresh token; keep the
            // stored one in that case.
            let refresh_token = tokens.refresh_token.unwrap_or(token.refresh_token);
            match persist_token(
                &state,
                endpoint_id,
                tokens.access_token,
                refresh_token,
                tokens.expires_in_seconds,
            )
            .await
            {
                Ok(()) => oauth_status_response(&state, endpoint_id).await,
                Err(response) => response,
            }
        }
        Err(error @ ChatgptOAuthError::InvalidGrant(_)) => {
            // Revoked/expired refresh token: drop the stored credential so the
            // derived plan falls back to platform_api_key.
            if let Err(err) = state
                .config_repository
                .clear_endpoint_oauth_token(endpoint_id)
                .await
            {
                return internal(&state, err);
            }
            publish_token_snapshot(&state).await;
            oauth_error_response(&state, error)
        }
        Err(error) => oauth_error_response(&state, error),
    }
}

pub(super) async fn oauth_clear(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return endpoint_not_found(),
        Err(err) => return internal(&state, err),
    }
    match state
        .config_repository
        .clear_endpoint_oauth_token(endpoint_id)
        .await
    {
        Ok(()) => {
            publish_token_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => internal(&state, err),
    }
}
