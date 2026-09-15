use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;

use super::mcp::{MCP_PROVIDER_CONTEXT7, MCP_PROVIDER_FIRECRAWL, MCP_PROVIDER_MINIMAX};

/// Canonical persisted provider id derived from an owning MCP server. Generic,
/// legacy (NULL/blank), and unknown values all resolve to `None`, so untyped
/// servers never trigger a provider-specific flow such as a Firecrawl balance
/// fetch (issue #296 Phase 3).
pub fn canonical_mcp_provider_kind(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(MCP_PROVIDER_CONTEXT7) => Some(MCP_PROVIDER_CONTEXT7),
        Some(MCP_PROVIDER_FIRECRAWL) => Some(MCP_PROVIDER_FIRECRAWL),
        Some(MCP_PROVIDER_MINIMAX) => Some(MCP_PROVIDER_MINIMAX),
        _ => None,
    }
}

/// Credential secret selected for an out-of-band provider API call. The type
/// is never serialized and its `Debug` output redacts the secret.
#[derive(Clone, FromRow)]
pub struct McpProviderSecret {
    pub credential_id: uuid::Uuid,
    pub secret: String,
}

impl std::fmt::Debug for McpProviderSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpProviderSecret")
            .field("credential_id", &self.credential_id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct McpCredential {
    pub credential_id: uuid::Uuid,
    pub server_id: uuid::Uuid,
    pub credential_label: String,
    pub secret: String,
    pub position: i32,
    pub enabled: bool,
    pub provider_kind: Option<String>,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: f64,
    pub strict_mode: bool,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
    pub provider_remaining: Option<f64>,
    pub provider_synced_at: Option<DateTime<Utc>>,
    pub provider_reset_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl McpCredential {
    pub fn is_exhausted(&self) -> bool {
        self.provider_remaining
            .is_some_and(|remaining| remaining <= 0.0)
    }

    pub fn is_in_cooldown(&self, now: DateTime<Utc>) -> bool {
        self.cooldown_until.is_some_and(|until| until > now)
    }

    fn masked_secret_preview(&self) -> String {
        const MASK: &str = "••••••••";
        if self.secret.chars().count() <= 8 {
            return MASK.to_string();
        }
        let tail: String = self.secret.chars().rev().take(4).collect();
        format!("{MASK}{}", tail.chars().rev().collect::<String>())
    }
}

/// Admin-API wire representation of a credential. The raw `secret` is
/// deliberately never serialized; only a masked preview is exposed.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpCredentialView {
    pub credential_id: uuid::Uuid,
    pub server_id: uuid::Uuid,
    pub credential_label: String,
    pub secret_preview: String,
    pub position: i32,
    pub enabled: bool,
    pub provider_kind: Option<String>,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: f64,
    pub strict_mode: bool,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
    pub provider_remaining: Option<f64>,
    pub provider_synced_at: Option<DateTime<Utc>>,
    pub provider_reset_at: Option<DateTime<Utc>>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<McpCredential> for McpCredentialView {
    fn from(credential: McpCredential) -> Self {
        let secret_preview = credential.masked_secret_preview();
        Self {
            credential_id: credential.credential_id,
            server_id: credential.server_id,
            credential_label: credential.credential_label,
            secret_preview,
            position: credential.position,
            enabled: credential.enabled,
            provider_kind: credential.provider_kind,
            daily_limit: credential.daily_limit,
            monthly_limit: credential.monthly_limit,
            default_cost: credential.default_cost,
            strict_mode: credential.strict_mode,
            billing_period_start: credential.billing_period_start,
            billing_period_end: credential.billing_period_end,
            provider_remaining: credential.provider_remaining,
            provider_synced_at: credential.provider_synced_at,
            provider_reset_at: credential.provider_reset_at,
            cooldown_until: credential.cooldown_until,
            last_error: credential.last_error,
            last_error_at: credential.last_error_at,
            created_at: credential.created_at,
            updated_at: credential.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_credential_provider_never_triggers_on_generic_or_legacy() {
        assert_eq!(canonical_mcp_provider_kind(None), None);
        assert_eq!(canonical_mcp_provider_kind(Some("")), None);
        assert_eq!(canonical_mcp_provider_kind(Some("  ")), None);
        assert_eq!(canonical_mcp_provider_kind(Some("generic")), None);
        assert_eq!(canonical_mcp_provider_kind(Some("legacy-unknown")), None);
        assert_eq!(
            canonical_mcp_provider_kind(Some(" firecrawl ")),
            Some(MCP_PROVIDER_FIRECRAWL)
        );
        assert_eq!(
            canonical_mcp_provider_kind(Some("context7")),
            Some(MCP_PROVIDER_CONTEXT7)
        );
        assert_eq!(
            canonical_mcp_provider_kind(Some("minimax")),
            Some(MCP_PROVIDER_MINIMAX)
        );
    }

    #[test]
    fn provider_secret_debug_redacts_the_secret() {
        let secret = McpProviderSecret {
            credential_id: uuid::Uuid::nil(),
            secret: "fc-secret-value".to_string(),
        };
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("fc-secret-value"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }
}
