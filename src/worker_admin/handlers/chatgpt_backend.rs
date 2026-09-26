//! Issue #599 R2c: the ChatGPT (Codex) private-interface mapping layer.
//!
//! Everything that knows about `chatgpt.com` private endpoints lives here: the
//! Codex backend URL, the ChatGPT auth headers, the Codex model table, the
//! display-only quota windows, and the OAuth token refresh used by the request
//! path. Interface drift therefore stays a single-file change.
//!
//! The runtime request path (`worker::runtime::ai`) reaches these helpers
//! through `worker_admin::chatgpt_backend`, the same way it already uses
//! `worker_admin::token_plan_cache`; no secret is ever logged or echoed.

use std::borrow::Cow;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE};
use chrono::{DateTime, Utc};
use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use uuid::Uuid;

use super::oauth_client::{
    CHATGPT_ISSUER, ChatgptOAuthError, OAuthTokenResponse, refresh_chatgpt_tokens,
};
use crate::db::{ConfigRepository, EndpointOAuthTokenSet};

/// Codex backend root (the `/codex/responses` path is appended by the mapper).
pub const CHATGPT_BACKEND_BASE_URL: &str = "https://chatgpt.com/backend-api";
/// Deployment override for the Codex backend root (integration tests and
/// air-gapped fronting). Read per call so tests can point it at a double.
pub const CHATGPT_BACKEND_URL_ENV: &str = "PROMPT_FERRY_CHATGPT_BACKEND_URL";
/// Codex backend Responses path (streaming turns).
pub const CHATGPT_CODEX_RESPONSES_PATH: &str = "/codex/responses";
/// Codex backend unary history-compaction path.
pub const CHATGPT_CODEX_COMPACT_PATH: &str = "/codex/responses/compact";
/// OAuth issuer override (login + token refresh), shared with R2b.
const CHATGPT_ISSUER_ENV: &str = "PROMPT_FERRY_CHATGPT_OAUTH_ISSUER";
/// Client marker mirrored from the R2b login flow (Codex/opencode contract).
const CHATGPT_ORIGINATOR: &str = "opencode";
/// Quota endpoint candidates, in preference order. `wham/usage` is what the
/// current Codex CLI calls; `codex/usage` is the older compatibility path.
const CHATGPT_USAGE_PATHS: [&str; 2] = ["/wham/usage", "/codex/usage"];
const DEFAULT_TOKEN_TTL_SECONDS: i64 = 3600;
const MIN_TOKEN_TTL_SECONDS: i64 = 60;
const MAX_TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;
const MESSAGE_LIMIT: usize = 240;

/// Default Codex backend model for any caller model the backend does not
/// accept (the subscription backend rejects non-Codex OpenAI models).
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.1-codex";
/// Codex backend model ids accepted by the ChatGPT subscription backend.
static CODEX_MODELS: [&str; 9] = [
    "gpt-5.2",
    "gpt-5.2-codex",
    "gpt-5.1",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5-codex",
    "gpt-5",
    "codex-mini-latest",
];
/// Reasoning-effort suffixes appended to a Codex model id by Codex-style
/// clients; they are folded into the base model (reasoning effort itself stays
/// in the request body).
const REASONING_EFFORT_SUFFIXES: [&str; 6] =
    ["-none", "-minimal", "-low", "-medium", "-high", "-xhigh"];

/// Codex backend root for this deployment.
pub fn chatgpt_backend_base_url() -> String {
    std::env::var(CHATGPT_BACKEND_URL_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| CHATGPT_BACKEND_BASE_URL.to_string())
}

/// Map a prepared Responses-native upstream path onto the Codex backend.
/// `None` means the path has no Codex backend equivalent (the caller must
/// report a clear protocol error instead of forwarding it).
pub fn chatgpt_codex_path(native_path: &str) -> Option<&'static str> {
    match native_path {
        "/v1/responses" => Some(CHATGPT_CODEX_RESPONSES_PATH),
        "/v1/responses/compact" => Some(CHATGPT_CODEX_COMPACT_PATH),
        _ => None,
    }
}

/// Full Codex backend URL for a prepared Responses-native path.
pub fn chatgpt_codex_url(native_path: &str) -> Option<String> {
    chatgpt_codex_path(native_path).map(|path| format!("{}{path}", chatgpt_backend_base_url()))
}

/// Normalize a caller model onto a Codex backend model id.
///
/// Known Codex ids (and their `-low`/`-high`-style reasoning-effort variants)
/// pass through with the suffix folded into the base id; anything else — other
/// OpenAI families and unknown names — maps to [`DEFAULT_CODEX_MODEL`] because
/// the subscription backend rejects them. A leading `provider/` segment is
/// dropped first.
pub fn normalize_codex_model(requested: &str) -> &'static str {
    let name = requested.trim();
    let name = name.rsplit('/').next().unwrap_or(name).trim();
    if let Some(known) = known_codex_model(name) {
        return known;
    }
    for suffix in REASONING_EFFORT_SUFFIXES {
        if let Some(base) = name.strip_suffix(suffix)
            && let Some(known) = known_codex_model(base)
        {
            return known;
        }
    }
    DEFAULT_CODEX_MODEL
}

fn known_codex_model(name: &str) -> Option<&'static str> {
    CODEX_MODELS
        .iter()
        .find(|model| model.eq_ignore_ascii_case(name))
        .copied()
}

/// Rewrite a Responses request body for the Codex backend: normalize `model`
/// and force the stateless `store: false` flag. Borrows the original bytes
/// when nothing changes (prefix-cache stable), so an already-normalized body
/// is forwarded byte-for-byte.
pub fn normalize_codex_request_body<'a>(body: &'a [u8]) -> Cow<'a, [u8]> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Cow::Borrowed(body);
    };
    let Some(object) = value.as_object_mut() else {
        return Cow::Borrowed(body);
    };
    let mut changed = false;
    if let Some(model) = object.get("model").and_then(Value::as_str) {
        let normalized = normalize_codex_model(model);
        if normalized != model {
            object.insert("model".to_string(), Value::String(normalized.to_string()));
            changed = true;
        }
    }
    if object.get("store") != Some(&Value::Bool(false)) {
        object.insert("store".to_string(), Value::Bool(false));
        changed = true;
    }
    if changed {
        Cow::Owned(serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec()))
    } else {
        Cow::Borrowed(body)
    }
}

/// ChatGPT account id carried by the access-token JWT (`chatgpt_account_id`,
/// top level or under the `https://api.openai.com/auth` claim). `None` when
/// the token is opaque or malformed; the request is still attempted.
pub fn codex_account_id_from_access_token(access_token: &str) -> Option<String> {
    let payload = access_token.split('.').nth(1)?;
    let mut padded = payload.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let bytes = URL_SAFE.decode(padded.as_bytes()).ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    account_id_from_claims(&value)
}

fn account_id_from_claims(payload: &Value) -> Option<String> {
    let direct = payload.get("chatgpt_account_id");
    let nested = payload
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"));
    direct
        .or(nested)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// ChatGPT subscription credential resolved for one request. `refreshed`
/// tracks whether the access token already came from a refresh, so the 401
/// retry never refreshes twice for the same turn.
#[derive(Clone)]
pub struct CodexAuth {
    pub access_token: String,
    pub account_id: Option<String>,
    pub refreshed: bool,
}

impl CodexAuth {
    pub fn new(access_token: String, refreshed: bool) -> Self {
        Self {
            account_id: codex_account_id_from_access_token(&access_token),
            access_token,
            refreshed,
        }
    }
}

impl std::fmt::Debug for CodexAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexAuth")
            .field("access_token", &"<redacted>")
            .field("account_id", &self.account_id)
            .field("refreshed", &self.refreshed)
            .finish()
    }
}

/// ChatGPT auth headers for backend requests: bearer access token, the
/// account binding header (when the token claims it), and the client marker.
pub fn with_codex_headers(
    builder: RequestBuilder,
    access_token: &str,
    account_id: Option<&str>,
) -> RequestBuilder {
    let builder = builder
        .bearer_auth(access_token)
        .header("originator", CHATGPT_ORIGINATOR)
        .header(
            reqwest::header::USER_AGENT,
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION")),
        );
    match account_id.filter(|value| !value.is_empty()) {
        Some(account_id) => builder.header("chatgpt-account-id", account_id),
        None => builder,
    }
}

/// One display-only ChatGPT rate-limit window (percent based).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatgptQuotaWindow {
    pub used_percent: Option<f64>,
    pub limit_window_seconds: Option<i64>,
    pub reset_after_seconds: Option<i64>,
    pub reset_at: Option<DateTime<Utc>>,
}

/// Display-only ChatGPT subscription quota snapshot. Never feeds routing:
/// the token-plan weight cache and quota failover are untouched.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatgptQuota {
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub primary: Option<ChatgptQuotaWindow>,
    pub secondary: Option<ChatgptQuotaWindow>,
    pub has_credits: Option<bool>,
    pub unlimited_credits: Option<bool>,
    pub credits_balance: Option<String>,
}

#[derive(Debug)]
pub enum ChatgptBackendError {
    /// No stored token for the endpoint (or a repository failure).
    NotConfigured(String),
    /// The refresh grant was rejected; the stored token has been cleared.
    InvalidGrant(String),
    /// The ChatGPT backend rejected or could not serve the request.
    Upstream {
        status: Option<u16>,
        message: String,
    },
}

impl std::fmt::Display for ChatgptBackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(message) | Self::InvalidGrant(message) => {
                formatter.write_str(message)
            }
            Self::Upstream { status, message } => match status {
                Some(status) => write!(formatter, "{message} (HTTP {status})"),
                None => formatter.write_str(message),
            },
        }
    }
}

impl std::error::Error for ChatgptBackendError {}

impl From<ChatgptOAuthError> for ChatgptBackendError {
    fn from(error: ChatgptOAuthError) -> Self {
        match error {
            ChatgptOAuthError::InvalidGrant(message) => Self::InvalidGrant(message),
            other => Self::Upstream {
                status: None,
                message: truncate(&other.to_string()),
            },
        }
    }
}

/// ChatGPT OAuth issuer for this deployment (login + token refresh).
pub fn chatgpt_oauth_issuer() -> String {
    std::env::var(CHATGPT_ISSUER_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| CHATGPT_ISSUER.to_string())
}

/// Access-token expiry for a `expires_in` value, using the same clamp as the
/// R2b login persistence (60s ..= 30d, default 1h).
pub fn token_expiry(expires_in_seconds: Option<u64>) -> DateTime<Utc> {
    let ttl_seconds = expires_in_seconds
        .and_then(|seconds| i64::try_from(seconds).ok())
        .unwrap_or(DEFAULT_TOKEN_TTL_SECONDS)
        .clamp(MIN_TOKEN_TTL_SECONDS, MAX_TOKEN_TTL_SECONDS);
    Utc::now() + chrono::Duration::seconds(ttl_seconds)
}

pub fn access_token_expired(expires_at: Option<DateTime<Utc>>) -> bool {
    expires_at.is_some_and(|expires_at| expires_at <= Utc::now())
}

/// Fetch the display-only subscription quota. Prefers `wham/usage` and falls
/// back to the older `codex/usage` path when the endpoint moved.
pub async fn fetch_chatgpt_quota(
    client: &Client,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<ChatgptQuota, ChatgptBackendError> {
    let base = chatgpt_backend_base_url();
    let mut last_status = None;
    for path in CHATGPT_USAGE_PATHS {
        let response = with_codex_headers(
            client
                .get(format!("{base}{path}"))
                .timeout(QUOTA_REQUEST_TIMEOUT),
            access_token,
            account_id,
        )
        .send()
        .await
        .map_err(|error| ChatgptBackendError::Upstream {
            status: error.status().map(|status| status.as_u16()),
            message: truncate(&error.to_string()),
        })?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            let value = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
            return Ok(parse_chatgpt_quota(&value));
        }
        if matches!(status.as_u16(), 404 | 405) {
            last_status = Some(status);
            continue;
        }
        return Err(ChatgptBackendError::Upstream {
            status: Some(status.as_u16()),
            message: if body.trim().is_empty() {
                format!("ChatGPT usage request failed with HTTP {status}")
            } else {
                truncate(body.trim())
            },
        });
    }
    Err(ChatgptBackendError::Upstream {
        status: last_status.map(|status| status.as_u16()),
        message: "ChatGPT usage endpoint is not available".to_string(),
    })
}

/// Parse the `wham/usage` / `codex/usage` payload. Missing sections degrade to
/// `None` instead of failing the display.
pub fn parse_chatgpt_quota(value: &Value) -> ChatgptQuota {
    let rate_limit = value.get("rate_limit");
    let credits = value.get("credits");
    ChatgptQuota {
        plan_type: string_field(value, "plan_type"),
        limit_reached: rate_limit
            .and_then(|rate_limit| rate_limit.get("limit_reached"))
            .and_then(Value::as_bool),
        primary: rate_limit
            .and_then(|rate_limit| rate_limit.get("primary_window"))
            .map(parse_quota_window),
        secondary: rate_limit
            .and_then(|rate_limit| rate_limit.get("secondary_window"))
            .map(parse_quota_window),
        has_credits: credits
            .and_then(|credits| credits.get("has_credits"))
            .and_then(Value::as_bool),
        unlimited_credits: credits
            .and_then(|credits| credits.get("unlimited"))
            .and_then(Value::as_bool),
        credits_balance: credits
            .and_then(|credits| credits.get("balance"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn parse_quota_window(value: &Value) -> ChatgptQuotaWindow {
    ChatgptQuotaWindow {
        used_percent: value.get("used_percent").and_then(Value::as_f64),
        limit_window_seconds: value.get("limit_window_seconds").and_then(Value::as_i64),
        reset_after_seconds: value.get("reset_after_seconds").and_then(Value::as_i64),
        reset_at: value
            .get("reset_at")
            .and_then(Value::as_i64)
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Refresh the stored OAuth token for an endpoint and persist the rotated
/// credential. A rejected grant clears the stored token (the derived plan
/// falls back to `platform_api_key`).
pub async fn refresh_stored_endpoint_token(
    repository: &ConfigRepository,
    client: &Client,
    endpoint_id: Uuid,
) -> Result<OAuthTokenResponse, ChatgptBackendError> {
    let stored = repository
        .get_endpoint_oauth_token(endpoint_id)
        .await
        .map_err(|error| {
            ChatgptBackendError::NotConfigured(truncate(&format!(
                "failed to read the stored ChatGPT OAuth token: {error}"
            )))
        })?
        .ok_or_else(|| {
            ChatgptBackendError::NotConfigured(
                "no ChatGPT OAuth token is stored for this endpoint".to_string(),
            )
        })?;
    let tokens = match refresh_chatgpt_tokens(
        client,
        &chatgpt_oauth_issuer(),
        &stored.refresh_token,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error @ ChatgptOAuthError::InvalidGrant(_)) => {
            let _ = repository.clear_endpoint_oauth_token(endpoint_id).await;
            return Err(ChatgptBackendError::InvalidGrant(truncate(
                &error.to_string(),
            )));
        }
        Err(error) => return Err(error.into()),
    };
    // A refresh response may omit a rotated refresh token; keep the stored one.
    let refresh_token = tokens
        .refresh_token
        .clone()
        .unwrap_or_else(|| stored.refresh_token.clone());
    repository
        .set_endpoint_oauth_token(
            endpoint_id,
            Some(EndpointOAuthTokenSet {
                access_token: tokens.access_token.clone(),
                refresh_token,
                expires_at: Some(token_expiry(tokens.expires_in_seconds)),
            }),
        )
        .await
        .map_err(|error| {
            ChatgptBackendError::NotConfigured(truncate(&format!(
                "failed to persist the refreshed ChatGPT OAuth token: {error}"
            )))
        })?;
    Ok(tokens)
}

/// Bounded per-request timeout for the quota fetch (display-only path).
pub const QUOTA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn truncate(message: &str) -> String {
    if message.chars().count() <= MESSAGE_LIMIT {
        return message.to_string();
    }
    let mut truncated = String::new();
    for character in message.chars().take(MESSAGE_LIMIT - 3) {
        truncated.push(character);
    }
    truncated.push_str("...");
    truncated
}
