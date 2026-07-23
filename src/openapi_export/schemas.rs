use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub(super) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize, ToSchema)]
pub(super) struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(super) struct EndpointSettingResponse {
    pub endpoint_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum RedactionInputKindSchema {
    Text,
    GitDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionRulesSchema {
    pub secret: bool,
    pub domain: bool,
    pub url: bool,
    pub email: bool,
    pub ip: bool,
    pub cidr: bool,
    pub phone: bool,
    pub person: bool,
    pub organization: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum CustomStringMatchSchema {
    Exact,
    Contains,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum CustomStringScopeSchema {
    Text,
    Line,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct CustomStringRuleSchema {
    pub pattern: String,
    pub match_type: CustomStringMatchSchema,
    pub scope: CustomStringScopeSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionConfigSchema {
    pub enabled: bool,
    pub rules: RedactionRulesSchema,
    pub custom_strings: Vec<CustomStringRuleSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum RedactionScopeSchema {
    Global,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionSettingResponseSchema {
    pub scope: RedactionScopeSchema,
    pub user_id: Option<i64>,
    pub config: RedactionConfigSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionCustomStringRuleRowSchema {
    pub array_index: i64,
    pub pattern: String,
    pub match_type: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionCustomStringRulePageResponseSchema {
    pub items: Vec<RedactionCustomStringRuleRowSchema>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionPreviewRequestSchema {
    pub text: String,
    pub input_kind: RedactionInputKindSchema,
    pub enabled: bool,
    pub rules: RedactionRulesSchema,
    pub custom_strings: Vec<CustomStringRuleSchema>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionFindingSchema {
    pub kind: String,
    pub source: String,
    pub match_text: String,
    pub normalized_key: String,
    pub confidence: f64,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct AppliedReplacementSchema {
    pub kind: String,
    pub replacement: String,
    pub strategy: String,
    pub display_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionStatsSchema {
    pub total_findings: usize,
    pub applied_replacements: usize,
    pub dropped_findings: usize,
    pub llm_configured: bool,
    pub llm_request_failed: bool,
    pub llm_candidates_accepted: usize,
    pub llm_candidates_rejected: usize,
    pub llm_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionPreviewSchema {
    pub redacted_text: String,
    pub findings: Vec<RedactionFindingSchema>,
    pub applied_replacements: Vec<AppliedReplacementSchema>,
    pub stats: RedactionStatsSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(super) struct RedactionPreviewResponseSchema {
    pub preview: RedactionPreviewSchema,
}
