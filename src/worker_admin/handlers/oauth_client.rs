//! Issue #599 R2b: ChatGPT (Codex) OAuth protocol client.
//!
//! Mirrors the opencode / Codex login contract (PKCE S256 authorization code
//! plus device-code). Tokens go straight into the R2a envelope, so token types
//! carry no `Debug`/`Serialize` and every upstream message is truncated.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::Client;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::truncate_message;

pub(super) const CHATGPT_ISSUER: &str = "https://auth.openai.com";
pub(super) const BROWSER_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CHATGPT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Clone)]
pub struct PkceCodes {
    pub verifier: String,
    pub challenge: String,
}

/// RFC 7636 verifier (43 unreserved characters) plus its S256 challenge.
pub fn generate_pkce() -> PkceCodes {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = [0_u8; 43];
    rand::rng().fill_bytes(&mut bytes);
    let verifier = bytes
        .iter()
        .map(|byte| CHARSET[*byte as usize % CHARSET.len()] as char)
        .collect::<String>();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    PkceCodes {
        verifier,
        challenge,
    }
}

pub fn random_state() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Authorize URL for the browser login, mirroring the Codex / opencode client.
pub fn authorize_url(
    issuer: &str,
    redirect_uri: &str,
    pkce: &PkceCodes,
    oauth_state: &str,
) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CHATGPT_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", oauth_state),
        ("originator", "opencode"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{issuer}/oauth/authorize?{query}")
}

#[derive(Debug)]
pub enum ChatgptOAuthError {
    RateLimited(String),
    InvalidGrant(String),
    AuthorizationDenied(String),
    InvalidRedirect(String),
    Upstream {
        status: Option<u16>,
        message: String,
    },
}

impl ChatgptOAuthError {
    /// Terminal errors discard the pending login; transient ones keep it.
    pub(super) fn is_terminal_flow_error(&self) -> bool {
        matches!(self, Self::InvalidGrant(_) | Self::AuthorizationDenied(_))
    }
}

impl std::fmt::Display for ChatgptOAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited(message)
            | Self::InvalidGrant(message)
            | Self::AuthorizationDenied(message)
            | Self::InvalidRedirect(message) => formatter.write_str(message),
            Self::Upstream { status, message } => match status {
                Some(status) => write!(formatter, "{message} (HTTP {status})"),
                None => formatter.write_str(message),
            },
        }
    }
}

#[derive(Clone)]
pub struct DeviceAuthorization {
    pub device_auth_id: String,
    pub user_code: String,
    pub interval_seconds: u64,
}

pub enum DevicePollOutcome {
    Pending,
    Authorized {
        authorization_code: String,
        code_verifier: String,
    },
    Denied(String),
}

#[derive(Clone)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in_seconds: Option<u64>,
}

pub async fn request_device_authorization(
    client: &Client,
    issuer: &str,
) -> Result<DeviceAuthorization, ChatgptOAuthError> {
    let response = client
        .post(format!("{issuer}/api/accounts/deviceauth/usercode"))
        .json(&serde_json::json!({ "client_id": CHATGPT_CLIENT_ID }))
        .send()
        .await
        .map_err(transport_error)?;
    let value = decode_response(response, false).await?;
    let (Some(device_auth_id), Some(user_code)) = (
        non_empty_field(&value, "device_auth_id"),
        non_empty_field(&value, "user_code"),
    ) else {
        return Err(missing_field("device_auth_id/user_code"));
    };
    let interval_seconds = value
        .get("interval")
        .and_then(parse_interval)
        .unwrap_or(5)
        .max(1);
    Ok(DeviceAuthorization {
        device_auth_id,
        user_code,
        interval_seconds,
    })
}

pub async fn poll_device_authorization(
    client: &Client,
    issuer: &str,
    device: &DeviceAuthorization,
) -> Result<DevicePollOutcome, ChatgptOAuthError> {
    let response = client
        .post(format!("{issuer}/api/accounts/deviceauth/token"))
        .json(&serde_json::json!({
            "device_auth_id": device.device_auth_id,
            "user_code": device.user_code,
        }))
        .send()
        .await
        .map_err(transport_error)?;
    let status = response.status().as_u16();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    match status {
        200 => {
            let (Some(authorization_code), Some(code_verifier)) = (
                non_empty_field(&value, "authorization_code"),
                non_empty_field(&value, "code_verifier"),
            ) else {
                return Err(missing_field("authorization_code/code_verifier"));
            };
            Ok(DevicePollOutcome::Authorized {
                authorization_code,
                code_verifier,
            })
        }
        // ChatGPT keeps returning 403 (or 404) until the code is confirmed.
        403 | 404 => Ok(DevicePollOutcome::Pending),
        status if status == 429 || status >= 500 => Err(classify_error(status, &value)),
        _ => Ok(DevicePollOutcome::Denied(
            error_message(&value)
                .unwrap_or_else(|| format!("ChatGPT rejected the device code (HTTP {status})")),
        )),
    }
}

pub async fn exchange_authorization_code(
    client: &Client,
    issuer: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<OAuthTokenResponse, ChatgptOAuthError> {
    let value = post_form(
        client,
        &format!("{issuer}/oauth/token"),
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CHATGPT_CLIENT_ID),
            ("code_verifier", code_verifier),
        ],
    )
    .await?;
    parse_token_response(value)
}

pub async fn refresh_chatgpt_tokens(
    client: &Client,
    issuer: &str,
    refresh_token: &str,
) -> Result<OAuthTokenResponse, ChatgptOAuthError> {
    let value = post_form(
        client,
        &format!("{issuer}/oauth/token"),
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CHATGPT_CLIENT_ID),
        ],
    )
    .await?;
    parse_token_response(value)
}

/// Extract the authorization code from the URL the browser landed on after
/// ChatGPT redirected to `redirect_uri` (or a bare query string), rejecting it
/// unless it carries the pending login state.
pub fn parse_redirect_url(
    redirect_url: &str,
    expected_state: &str,
) -> Result<String, ChatgptOAuthError> {
    let trimmed = redirect_url.trim();
    let query = match reqwest::Url::parse(trimmed) {
        Ok(url) => url.query().unwrap_or_default().to_string(),
        Err(_) => trimmed.trim_start_matches('?').to_string(),
    };
    let parsed = reqwest::Url::parse(&format!("http://localhost/auth/callback?{query}"))
        .map_err(|_| invalid_redirect("pasted redirect URL is not a valid URL"))?;
    let params = parsed
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    if let Some(error) = params.get("error") {
        let message = params
            .get("error_description")
            .map(|description| description.to_string())
            .unwrap_or_else(|| error.to_string());
        return Err(ChatgptOAuthError::AuthorizationDenied(message));
    }
    let Some(code) = params
        .get("code")
        .map(|code| code.trim())
        .filter(|code| !code.is_empty())
    else {
        return Err(invalid_redirect(
            "pasted redirect URL is missing the authorization code",
        ));
    };
    if params.get("state").map(|state| state.as_ref()) != Some(expected_state) {
        return Err(invalid_redirect(
            "pasted redirect URL does not match the pending login state",
        ));
    }
    Ok(code.to_string())
}

fn invalid_redirect(message: &str) -> ChatgptOAuthError {
    ChatgptOAuthError::InvalidRedirect(message.to_string())
}

fn missing_field(fields: &str) -> ChatgptOAuthError {
    ChatgptOAuthError::Upstream {
        status: None,
        message: format!("ChatGPT OAuth response is missing {fields}"),
    }
}

fn parse_token_response(value: Value) -> Result<OAuthTokenResponse, ChatgptOAuthError> {
    let Some(access_token) = non_empty_field(&value, "access_token") else {
        return Err(missing_field("access_token"));
    };
    Ok(OAuthTokenResponse {
        access_token,
        refresh_token: non_empty_field(&value, "refresh_token"),
        expires_in_seconds: value.get("expires_in").and_then(Value::as_u64),
    })
}

fn non_empty_field(value: &Value, field: &str) -> Option<String> {
    let field = value.get(field)?.as_str()?.trim();
    (!field.is_empty()).then(|| field.to_string())
}

fn parse_interval(value: &Value) -> Option<u64> {
    let from_string = value.as_str().and_then(|raw| raw.trim().parse().ok());
    value.as_u64().or(from_string)
}

fn error_message(body: &Value) -> Option<String> {
    let error = body.get("error")?;
    error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| body.get("error_description").and_then(Value::as_str))
        .or_else(|| error.as_str())
        .map(str::to_string)
}

fn error_code(body: &Value) -> Option<String> {
    match body.get("error")? {
        Value::String(code) => Some(code.clone()),
        error => error.get("code")?.as_str().map(str::to_string),
    }
}

fn classify_error(status: u16, body: &Value) -> ChatgptOAuthError {
    let message = error_message(body)
        .unwrap_or_else(|| format!("ChatGPT OAuth request failed with HTTP {status}"));
    if status == 429 {
        return ChatgptOAuthError::RateLimited(message);
    }
    ChatgptOAuthError::Upstream {
        status: Some(status),
        message,
    }
}

/// Token-endpoint classification: 400/401 means the grant was rejected.
fn classify_token_error(status: u16, body: &Value) -> ChatgptOAuthError {
    if matches!(status, 400 | 401) && error_code(body).is_none_or(|code| code == "invalid_grant") {
        let message = error_message(body)
            .unwrap_or_else(|| "ChatGPT rejected the authorization grant".to_string());
        return ChatgptOAuthError::InvalidGrant(message);
    }
    classify_error(status, body)
}

fn transport_error(error: reqwest::Error) -> ChatgptOAuthError {
    ChatgptOAuthError::Upstream {
        status: error.status().map(|status| status.as_u16()),
        message: truncate_message(&error.to_string()),
    }
}

async fn post_form(
    client: &Client,
    url: &str,
    params: &[(&str, &str)],
) -> Result<Value, ChatgptOAuthError> {
    let body = params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    let response = client
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(transport_error)?;
    decode_response(response, true).await
}

async fn decode_response(
    response: reqwest::Response,
    token_endpoint: bool,
) -> Result<Value, ChatgptOAuthError> {
    let status = response.status().as_u16();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(value);
    }
    Err(if token_endpoint {
        classify_token_error(status, &value)
    } else {
        classify_error(status, &value)
    })
}
