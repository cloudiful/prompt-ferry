use super::*;
use crate::db::{MCP_AUTH_MODE_OAUTH, McpServer, oauth_owner_matches};
use crate::keys::hash_client_key;
use crate::mcp::oauth::{McpOAuthError, McpOAuthService, McpOAuthState};
use crate::worker_admin_types::{
    McpOAuthCallbackQuery, McpOAuthRefreshResponse, McpOAuthStartResponse, McpOAuthStatusResponse,
};

/// A fixed, static result page. The authorization server's query parameters,
/// any error text and any token material are never reflected into the body.
const OAUTH_CALLBACK_HTML: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>MCP authorization</title></head><body><p>The MCP authorization request has finished. You can close this window and return to prompt-ferry.</p></body></html>";

fn oauth_callback_response() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        OAUTH_CALLBACK_HTML,
    )
        .into_response()
}

#[allow(clippy::result_large_err)]
fn oauth_service(state: &AdminState) -> Result<std::sync::Arc<McpOAuthService>, Response> {
    state.mcp_oauth.clone().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mcp_oauth_unavailable",
            "personal MCP OAuth is not configured",
        )
    })
}

fn oauth_error_response(err: McpOAuthError) -> Response {
    let status = match &err {
        McpOAuthError::Forbidden => StatusCode::FORBIDDEN,
        McpOAuthError::NotFound => StatusCode::NOT_FOUND,
        McpOAuthError::InvalidState => StatusCode::BAD_REQUEST,
        McpOAuthError::AuthorizationRequired | McpOAuthError::RefreshInProgress => {
            StatusCode::CONFLICT
        }
        McpOAuthError::Disabled(_) => StatusCode::CONFLICT,
        McpOAuthError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        McpOAuthError::Upstream(_) => StatusCode::BAD_GATEWAY,
    };
    error(status, err.code(), &err.message())
}

/// Load the server and enforce the owner defence *before* any network access.
///
/// A personal OAuth server may only be managed by its owner, admins included.
#[allow(clippy::result_large_err)]
async fn load_owned_server(
    state: &AdminState,
    user: &SessionUser,
    server_id: Uuid,
) -> Result<McpServer, Response> {
    let server = state
        .config_repository
        .get_mcp_server(server_id)
        .await
        .map_err(|err| internal(state, err))?;
    let Some(server) = server else {
        return Err(error(
            StatusCode::NOT_FOUND,
            "not_found",
            "mcp server not found",
        ));
    };
    if server.auth_mode == MCP_AUTH_MODE_OAUTH {
        if !oauth_owner_matches(&server, user.user_id) {
            return Err(error(
                StatusCode::FORBIDDEN,
                "forbidden",
                "only the server owner may manage personal MCP OAuth",
            ));
        }
    } else if !user.is_admin && server.owner_user_id != Some(user.user_id) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "not_found",
            "mcp server not found",
        ));
    }
    Ok(server)
}

#[allow(clippy::result_large_err)]
fn require_oauth_server(server: &McpServer) -> Result<(), Response> {
    if server.auth_mode != MCP_AUTH_MODE_OAUTH {
        return Err(error(
            StatusCode::CONFLICT,
            "mcp_oauth_disabled",
            "this MCP server does not use OAuth",
        ));
    }
    if server.transport != crate::db::MCP_OAUTH_SUPPORTED_TRANSPORT {
        return Err(error(
            StatusCode::CONFLICT,
            "mcp_oauth_disabled",
            "personal MCP OAuth supports only the http transport",
        ));
    }
    Ok(())
}

fn session_binding(headers: &HeaderMap) -> String {
    session_id(headers).map(hash_client_key).unwrap_or_default()
}

pub(super) async fn get_mcp_oauth_status(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let server = match load_owned_server(&state, &user, server_id).await {
        Ok(server) => server,
        Err(response) => return response,
    };
    if server.auth_mode != MCP_AUTH_MODE_OAUTH {
        return Json(McpOAuthStatusResponse {
            status: McpOAuthState::Disconnected.as_str().to_string(),
            scopes: Vec::new(),
            expires_at: None,
            reason: Some("this MCP server does not use OAuth".to_string()),
        })
        .into_response();
    }
    let service = match oauth_service(&state) {
        Ok(service) => service,
        Err(response) => return response,
    };
    match service.status(&server, user.user_id).await {
        Ok(status) => Json(McpOAuthStatusResponse {
            status: status.state.as_str().to_string(),
            scopes: status.scopes,
            expires_at: status.expires_at,
            reason: status.reason,
        })
        .into_response(),
        Err(err) => oauth_error_response(err),
    }
}

pub(super) async fn start_mcp_oauth(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let server = match load_owned_server(&state, &user, server_id).await {
        Ok(server) => server,
        Err(response) => return response,
    };
    if let Err(response) = require_oauth_server(&server) {
        return response;
    }
    let service = match oauth_service(&state) {
        Ok(service) => service,
        Err(response) => return response,
    };
    match service
        .start(&server, user.user_id, &session_binding(&headers))
        .await
    {
        Ok(authorization_url) => Json(McpOAuthStartResponse { authorization_url }).into_response(),
        Err(err) => oauth_error_response(err),
    }
}

pub(super) async fn mcp_oauth_callback(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Query(query): Query<McpOAuthCallbackQuery>,
) -> Response {
    let mut outcome: Result<(), McpOAuthError> = Err(McpOAuthError::Forbidden);
    if let Ok(user) = current_user(&state, &headers).await {
        match load_owned_server(&state, &user, server_id).await {
            Ok(server) if server.auth_mode == MCP_AUTH_MODE_OAUTH => match oauth_service(&state) {
                Ok(service) => match (query.code.as_deref(), query.state.as_deref()) {
                    (Some(code), Some(state_value))
                        if !code.trim().is_empty() && !state_value.trim().is_empty() =>
                    {
                        outcome = service
                            .callback(
                                &server,
                                user.user_id,
                                &session_binding(&headers),
                                code,
                                state_value,
                                query.iss.as_deref(),
                            )
                            .await;
                    }
                    _ => outcome = Err(McpOAuthError::InvalidState),
                },
                Err(_) => outcome = Err(McpOAuthError::Unavailable),
            },
            Ok(_) => {
                outcome = Err(McpOAuthError::Disabled(
                    "this MCP server does not use OAuth".to_string(),
                ))
            }
            Err(_) => outcome = Err(McpOAuthError::NotFound),
        }
    }
    match &outcome {
        Ok(()) => tracing::info!(
            category = "mcp_oauth",
            event = "callback_succeeded",
            server_id = %server_id,
            "personal MCP OAuth callback completed"
        ),
        Err(err) => tracing::warn!(
            category = "mcp_oauth",
            event = "callback_failed",
            server_id = %server_id,
            error_code = err.code(),
            "personal MCP OAuth callback failed"
        ),
    }
    // The same page is returned for success and failure: no error text, no
    // authorization server query values and no redirect.
    oauth_callback_response()
}

pub(super) async fn refresh_mcp_oauth(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let server = match load_owned_server(&state, &user, server_id).await {
        Ok(server) => server,
        Err(response) => return response,
    };
    if let Err(response) = require_oauth_server(&server) {
        return response;
    }
    let service = match oauth_service(&state) {
        Ok(service) => service,
        Err(response) => return response,
    };
    match service.refresh(&server, user.user_id).await {
        Ok(status) => Json(McpOAuthRefreshResponse {
            status: McpOAuthState::Connected.as_str().to_string(),
            scopes: status.scopes,
            expires_at: status.expires_at,
        })
        .into_response(),
        Err(err) => oauth_error_response(err),
    }
}

pub(super) async fn disconnect_mcp_oauth(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let server = match load_owned_server(&state, &user, server_id).await {
        Ok(server) => server,
        Err(response) => return response,
    };
    let service = match oauth_service(&state) {
        Ok(service) => service,
        Err(response) => return response,
    };
    match service.disconnect(&server, user.user_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => oauth_error_response(err),
    }
}

#[cfg(test)]
mod tests;
