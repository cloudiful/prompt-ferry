use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::{IntoParams, ToSchema};

/// Safe status of one personal MCP OAuth grant.
///
/// `status` is one of `connected`, `authorizing`, `reauth_required` or
/// `disconnected`. The response never carries a token, client secret, PKCE
/// verifier or callback code.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpOAuthStatusResponse {
    /// `connected` | `authorizing` | `reauth_required` | `disconnected`
    pub status: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Why personal OAuth is unavailable when the deployment or the server
    /// configuration cannot enable it.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpOAuthStartResponse {
    /// The authorization-server URL the owner must visit to grant access.
    pub authorization_url: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpOAuthRefreshResponse {
    /// `connected` | `reauth_required`
    pub status: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// The authorization server's callback query.
///
/// The values are single-use and are never logged or echoed back.
#[derive(Debug, Default, serde::Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct McpOAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// RFC 9207 authorization response issuer.
    pub iss: Option<String>,
}
