use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use super::mcp::{
    MCP_PROVIDER_CONTEXT7, MCP_PROVIDER_FIRECRAWL, MCP_PROVIDER_GENERIC, MCP_PROVIDER_MINIMAX,
};

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

/// Canonicalize a quota-group provider input. Known presets are kept, while
/// `generic`, blank, and NULL all collapse to NULL (the canonical untyped
/// value). Unknown legacy strings are preserved so existing rows stay readable
/// and do not silently change meaning.
pub fn canonical_quota_group_provider_kind(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(str::trim).unwrap_or("");
    if trimmed.is_empty() || trimmed == MCP_PROVIDER_GENERIC {
        return None;
    }
    match canonical_mcp_provider_kind(Some(trimmed)) {
        Some(known) => Some(known.to_string()),
        None => Some(trimmed.to_string()),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum QuotaUnit {
    Requests,
    Credits,
}

impl QuotaUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::Credits => "credits",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum QuotaPeriodKind {
    Day,
    Month,
}

impl QuotaPeriodKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct QuotaPeriod {
    pub kind: QuotaPeriodKind,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct McpQuotaGroup {
    pub group_id: uuid::Uuid,
    pub name: String,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub provider_kind: Option<String>,
    pub unit: String,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: f64,
    pub strict_mode: bool,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpQuotaGroupInput {
    pub name: String,
    pub scope: Option<String>,
    pub owner_user_id: Option<i64>,
    pub provider_kind: Option<String>,
    pub unit: Option<QuotaUnit>,
    pub daily_limit: Option<f64>,
    pub monthly_limit: Option<f64>,
    pub default_cost: Option<f64>,
    pub strict_mode: Option<bool>,
    pub billing_period_start: Option<DateTime<Utc>>,
    pub billing_period_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct McpCredential {
    pub credential_id: uuid::Uuid,
    pub server_id: uuid::Uuid,
    pub credential_label: String,
    pub secret: String,
    pub position: i32,
    pub enabled: bool,
    pub quota_group_id: Option<uuid::Uuid>,
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
    pub quota_group_id: Option<uuid::Uuid>,
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
            quota_group_id: credential.quota_group_id,
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

#[derive(Debug, Clone, FromRow)]
pub struct McpQuotaAccountRow {
    pub account_id: i64,
    pub group_id: uuid::Uuid,
    pub period_kind: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub used_units: f64,
    pub reserved_units: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpQuotaAccountSnapshot {
    pub account_id: i64,
    pub period: QuotaPeriod,
    pub used_units: f64,
    pub reserved_units: f64,
}

#[derive(Debug, Clone)]
pub struct QuotaReservation {
    pub reservation_id: i64,
    pub account_id: i64,
    pub credential_id: uuid::Uuid,
    pub request_id: uuid::Uuid,
    pub units: f64,
}

#[derive(Debug, Clone)]
pub struct QuotaGrant {
    pub credential: McpCredential,
    pub reservation: QuotaReservation,
    /// Additional account rows updated for the day dimension, when present.
    pub day_account: Option<McpQuotaAccountSnapshot>,
    pub month_account: Option<McpQuotaAccountSnapshot>,
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
    fn quota_group_provider_collapses_generic_and_keeps_unknown() {
        assert_eq!(canonical_quota_group_provider_kind(None), None);
        assert_eq!(canonical_quota_group_provider_kind(Some("")), None);
        assert_eq!(canonical_quota_group_provider_kind(Some("generic")), None);
        assert_eq!(
            canonical_quota_group_provider_kind(Some(" firecrawl ")),
            Some("firecrawl".to_string())
        );
        assert_eq!(
            canonical_quota_group_provider_kind(Some("custom-legacy")),
            Some("custom-legacy".to_string())
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
