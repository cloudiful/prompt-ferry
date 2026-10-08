//! Authentication for the relay's management API.
//!
//! The listener is loopback-only, which is where the boundary starts: the relay
//! does not publish a management surface on its public API routes, and the
//! public fallback keeps forwarding to the worker instead. What loopback cannot
//! decide is which local caller is the operator, so every control endpoint —
//! and every worker request proxied through this listener — additionally
//! requires the host-local management token.
//!
//! A token is presented either directly or exchanged for an opaque session id.
//! The session is in-memory and expires, so a restart ends it and a stolen
//! cookie is bounded; the token itself is never stored in the browser.

use std::time::Instant;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng as _;

use crate::relay::state::RemoteAddr;

use super::{
    dto::{RelayAuthResponse, RelayLoginRequest},
    state::{RelayAdminState, SESSION_TTL},
};

/// Cookie carrying the opaque management session id.
pub(super) const SESSION_COOKIE_NAME: &str = "prompt_ferry_relay_admin";

/// An opaque session id. It carries no structure a caller could act on, and the
/// token it replaces never reaches the browser at all.
fn new_session_id() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compare two secrets without leaking where they first differ.
///
/// Length still differs, which is not a secret here: the token is fixed-length
/// by construction and a caller can measure its own input.
pub(super) fn constant_time_eq(expected: &str, provided: &str) -> bool {
    let (expected, provided) = (expected.as_bytes(), provided.as_bytes());
    if expected.len() != provided.len() {
        return false;
    }
    expected
        .iter()
        .zip(provided)
        .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message }
        })),
    )
        .into_response()
}

/// Refuse a caller with no valid management credential.
pub(super) fn unauthorized(message: &str) -> Response {
    error(StatusCode::UNAUTHORIZED, "unauthorized", message)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    let prefix = format!("{SESSION_COOKIE_NAME}=");
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
}

/// Whether a session id is still live, dropping it when it has expired.
async fn session_is_live(state: &RelayAdminState, session_id: &str) -> bool {
    let mut sessions = state.sessions.lock().await;
    let now = Instant::now();
    sessions.retain(|_, issued| now.duration_since(*issued) < SESSION_TTL);
    sessions.contains_key(session_id)
}

/// Whether the request carries a valid management credential.
async fn is_authenticated(state: &RelayAdminState, headers: &HeaderMap) -> bool {
    if let Some(token) = bearer(headers) {
        return constant_time_eq(&state.admin_token(), token);
    }
    match session_cookie(headers) {
        Some(session_id) => session_is_live(state, session_id).await,
        None => false,
    }
}

/// Guard every control endpoint and every proxied worker request.
pub(super) async fn require_admin(
    State(state): State<RelayAdminState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if !peer_addr.0.ip().is_loopback() {
        tracing::warn!(
            peer = %peer_addr.0,
            "relay management request rejected: the management listener is loopback-only"
        );
        return error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "the relay management API is reachable from loopback only",
        );
    }
    if !is_authenticated(&state, &headers).await {
        tracing::warn!(
            peer = %peer_addr.0,
            "relay management request rejected: no valid management credential"
        );
        return unauthorized("relay management authentication required");
    }
    next.run(request).await
}

/// Render the session cookie.
///
/// The path is the management API rather than `/`, because the same listener
/// serves the page and the cookie has no business travelling with it.
fn session_cookie_value(session_id: &str) -> String {
    format!("{SESSION_COOKIE_NAME}={session_id}; Path=/api/v1; HttpOnly; SameSite=Strict")
}

fn cleared_session_cookie() -> String {
    format!("{SESSION_COOKIE_NAME}=; Path=/api/v1; Max-Age=0; HttpOnly; SameSite=Strict")
}

pub(super) async fn login(
    State(state): State<RelayAdminState>,
    Json(body): Json<RelayLoginRequest>,
) -> Response {
    if !constant_time_eq(&state.admin_token(), &body.admin_token) {
        tracing::warn!("relay management login rejected: invalid management token");
        return unauthorized("invalid management token");
    }
    let session_id = new_session_id();
    state
        .sessions
        .lock()
        .await
        .insert(session_id.clone(), Instant::now());
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session_cookie_value(&session_id))],
    )
        .into_response()
}

pub(super) async fn logout(State(state): State<RelayAdminState>, headers: HeaderMap) -> Response {
    if let Some(session_id) = session_cookie(&headers) {
        state.sessions.lock().await.remove(session_id);
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cleared_session_cookie())],
    )
        .into_response()
}

/// Whether this caller is authenticated, so the page can decide to ask for a token.
pub(super) async fn me(State(state): State<RelayAdminState>, headers: HeaderMap) -> Response {
    Json(RelayAuthResponse {
        authenticated: is_authenticated(&state, &headers).await,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{cleared_session_cookie, constant_time_eq, session_cookie, session_cookie_value};

    #[test]
    fn the_constant_time_comparison_still_compares_the_values() {
        assert!(constant_time_eq("secret-token", "secret-token"));
        assert!(!constant_time_eq("secret-token", "secret-tokeN"));
        assert!(!constant_time_eq("secret-token", "secret-token2"));
        assert!(!constant_time_eq("secret-token", ""));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn the_session_cookie_is_scoped_to_the_management_api() {
        // The management listener also serves the SPA, so the cookie must not
        // travel to every path the relay serves, and it must not be readable by
        // a script.
        let cookie = session_cookie_value("abc123");
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            cookie.parse().expect("a valid cookie header"),
        );

        assert_eq!(session_cookie(&headers), Some("abc123"));
        assert!(cookie.contains("Path=/api/v1"), "{cookie}");
        assert!(cookie.contains("HttpOnly"), "{cookie}");
        assert!(cookie.contains("SameSite=Strict"), "{cookie}");
        assert!(cleared_session_cookie().contains("Max-Age=0"));
    }

    #[test]
    fn an_unrelated_cookie_never_reads_as_a_session() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "prompt_ferry_session=abc; theme=dark"
                .parse()
                .expect("a valid cookie header"),
        );

        assert_eq!(session_cookie(&headers), None);
    }
}
