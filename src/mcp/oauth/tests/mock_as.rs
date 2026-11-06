//! A minimal in-process authorization server for the OAuth flow tests.
//!
//! It is only reachable through the test-only pinned-client injection: the
//! production client refuses loopback and plain-HTTP URLs, so this mock can
//! never be selected by a deployment flag.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::State as AxumState,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refresh {
    Rotate,
    InvalidGrant,
    Transient,
}

#[derive(Default)]
pub(super) struct Script {
    pub(super) token_requests: Vec<String>,
    pub(super) registration_bodies: Vec<serde_json::Value>,
    pub(super) access_tokens: Vec<String>,
    pub(super) refresh: VecDeque<Refresh>,
    pub(super) token_redirect: bool,
    pub(super) registration_without_secret: bool,
    pub(super) register_application_type: Option<String>,
    pub(super) register_token_endpoint_auth_method: Option<String>,
    /// The scope the token endpoint grants. It is `read-mcp` unless a test
    /// asks for a wider grant.
    pub(super) granted_scope: Option<String>,
    /// The scope a refresh response grants; defaults to `read-mcp`.
    pub(super) refresh_granted_scope: Option<String>,
    pub(super) advertise_offline_access: bool,
    pub(super) omit_refresh_scope: bool,
    /// Discovery advertises this issuer instead of the mock's own base URL.
    pub(super) issuer_override: Option<String>,
    /// Discovery advertises this authorization endpoint.
    pub(super) authorization_endpoint_override: Option<String>,
    /// Delay the token endpoint by this long before answering.
    pub(super) token_delay: Option<std::time::Duration>,
    /// Answer the token endpoint with a JSON body above the 1 MiB cap.
    pub(super) oversize_token_body: bool,
    next_access: usize,
    next_refresh: usize,
}

pub(super) struct MockAs {
    base: String,
    script: Arc<Mutex<Script>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockAs {
    pub(super) async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock authorization server");
        let address = listener.local_addr().expect("mock address");
        let base = format!("http://{}", address);
        let script = Arc::new(Mutex::new(Script::default()));
        let app = Router::new()
            .route("/mcp", get(mcp_probe))
            .route(
                "/.well-known/oauth-protected-resource",
                get(protected_resource_metadata),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(authorization_server_metadata),
            )
            .route("/register", post(register))
            .route("/token", post(token))
            .with_state(MockState {
                base: base.clone(),
                script: script.clone(),
            });
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base,
            script,
            handle,
        }
    }

    pub(super) fn base(&self) -> &str {
        &self.base
    }

    pub(super) fn mcp_url(&self) -> String {
        format!("{}/mcp?login", self.base)
    }

    pub(super) fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script.lock().expect("mock script")
    }

    pub(super) fn push_refresh(&self, outcome: Refresh) {
        self.script().refresh.push_back(outcome);
    }
}

impl Drop for MockAs {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[derive(Clone)]
struct MockState {
    base: String,
    script: Arc<Mutex<Script>>,
}

async fn mcp_probe(AxumState(state): AxumState<MockState>) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            format!(
                "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
                state.base
            ),
        )],
        "",
    )
        .into_response()
}

async fn protected_resource_metadata(AxumState(state): AxumState<MockState>) -> Response {
    axum::Json(serde_json::json!({
        "resource": format!("{}/mcp?login", state.base),
        "authorization_servers": [state.base],
        "scopes_supported": ["read-mcp"],
    }))
    .into_response()
}

async fn authorization_server_metadata(AxumState(state): AxumState<MockState>) -> Response {
    let issuer = state
        .script
        .lock()
        .expect("script")
        .issuer_override
        .clone()
        .unwrap_or_else(|| state.base.clone());
    let authorization_endpoint = state
        .script
        .lock()
        .expect("script")
        .authorization_endpoint_override
        .clone()
        .unwrap_or_else(|| format!("{}/authorize", state.base));
    let scopes = if state
        .script
        .lock()
        .expect("script")
        .advertise_offline_access
    {
        vec!["read-mcp", "offline_access"]
    } else {
        vec!["read-mcp"]
    };
    axum::Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": authorization_endpoint,
        "token_endpoint": format!("{}/token", state.base),
        "registration_endpoint": format!("{}/register", state.base),
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "authorization_response_iss_parameter_supported": true,
        "scopes_supported": scopes,
    }))
    .into_response()
}

async fn register(AxumState(state): AxumState<MockState>, body: String) -> Response {
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    {
        let mut script = state.script.lock().expect("script");
        script.registration_bodies.push(parsed.clone());
        script.register_application_type = parsed
            .get("application_type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        script.register_token_endpoint_auth_method = parsed
            .get("token_endpoint_auth_method")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        if script.registration_without_secret {
            return axum::Json(serde_json::json!({ "client_id": "public-client" })).into_response();
        }
    }
    axum::Json(serde_json::json!({
        "client_id": "client-123",
        "client_secret": "confidential-secret",
        "token_endpoint_auth_method": "client_secret_basic",
    }))
    .into_response()
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if name == key {
            Some(value.replace('+', " "))
        } else {
            None
        }
    })
}

async fn token(
    AxumState(state): AxumState<MockState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    let grant_type = form_value(&body, "grant_type").unwrap_or_default();
    let (token_delay, oversize_token_body) = {
        let mut script = state.script.lock().expect("script");
        script.token_requests.push(body.clone());
        (script.token_delay, script.oversize_token_body)
    };
    if let Some(delay) = token_delay {
        tokio::time::sleep(delay).await;
    }
    if oversize_token_body {
        let filler = "x".repeat(1024 * 1024 + 1);
        return axum::Json(serde_json::json!({
            "access_token": filler,
            "token_type": "Bearer",
            "expires_in": 3600,
        }))
        .into_response();
    }
    {
        let script = state.script.lock().expect("script");
        if script.token_redirect {
            return (
                StatusCode::FOUND,
                [(header::LOCATION, "https://attacker.example.test/token")],
                "",
            )
                .into_response();
        }
    }
    let has_basic_auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("Basic "));
    match grant_type.as_str() {
        "authorization_code" => {
            if !has_basic_auth
                || form_value(&body, "code").as_deref() != Some("good-code")
                || form_value(&body, "code_verifier").is_none()
                || !form_value(&body, "resource")
                    .unwrap_or_default()
                    .contains("login")
            {
                return token_error("invalid_grant");
            }
            let mut script = state.script.lock().expect("script");
            script.next_access += 1;
            let access = format!("access-token-{}", script.next_access);
            let refresh = format!("refresh-token-{}", script.next_access);
            script.access_tokens.push(access.clone());
            let scope = script
                .granted_scope
                .clone()
                .unwrap_or_else(|| "read-mcp".to_string());
            axum::Json(serde_json::json!({
                "access_token": access,
                "refresh_token": refresh,
                "token_type": "Bearer",
                "expires_in": 3600,
                "scope": scope,
                "vendor_field": { "id_token": "vendor-id-token" },
            }))
            .into_response()
        }
        "refresh_token" => {
            let outcome = state
                .script
                .lock()
                .expect("script")
                .refresh
                .pop_front()
                .unwrap_or(Refresh::Rotate);
            match outcome {
                Refresh::Rotate => {
                    let mut script = state.script.lock().expect("script");
                    script.next_refresh += 1;
                    let access = format!("refreshed-token-{}", script.next_refresh);
                    script.access_tokens.push(access.clone());
                    let scope = script
                        .refresh_granted_scope
                        .clone()
                        .unwrap_or_else(|| "read-mcp".to_string());
                    let mut response = serde_json::json!({
                        "access_token": access,
                        "refresh_token": format!("rotated-refresh-{}", script.next_refresh),
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "scope": scope,
                    });
                    if script.omit_refresh_scope {
                        response.as_object_mut().unwrap().remove("scope");
                    }
                    axum::Json(response).into_response()
                }
                Refresh::InvalidGrant => token_error("invalid_grant"),
                Refresh::Transient => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({ "error": "server_error" })),
                )
                    .into_response(),
            }
        }
        _ => token_error("unsupported_grant_type"),
    }
}

fn token_error(code: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({ "error": code })),
    )
        .into_response()
}
