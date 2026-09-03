use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    protocol::RelayIpPolicy,
    redact::{RedactionConfig, RedactionPreviewRequest, RedactionPreviewResponse},
};

pub use crate::raw_payload_store::{
    RawObjectStoreBackend, RawObjectStoreConfig, RawObjectStorePersisted,
    RawObjectStoreSettingsResponse,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RedactionScope {
    Global,
    User,
}

#[derive(Debug, Deserialize)]
pub struct RedactionSettingQuery {
    pub scope: Option<RedactionScope>,
    pub user_id: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RedactionRulePageQuery {
    pub scope: Option<RedactionScope>,
    pub user_id: Option<i64>,
    pub first: Option<i64>,
    pub rows: Option<i64>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactionSettingResponse {
    pub scope: RedactionScope,
    pub user_id: Option<i64>,
    pub config: RedactionConfig,
}

#[derive(Debug, Deserialize)]
pub struct RedactionSettingRequest(pub RedactionConfig);

#[derive(Debug, Deserialize)]
pub struct RedactionPreviewRequestBody(pub RedactionPreviewRequest);

#[derive(Debug, Clone, Serialize)]
pub struct RedactionPreviewResponseBody {
    pub preview: RedactionPreviewResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RedactionCustomStringRuleRow {
    pub array_index: i64,
    pub pattern: String,
    pub match_type: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RedactionCustomStringRulePageResponse {
    pub items: Vec<RedactionCustomStringRuleRow>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestContentLoggingMode {
    Off,
    NormalizedOnly,
    NormalizedAndRaw,
}

impl RequestContentLoggingMode {
    pub fn captures_normalized(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn captures_raw(self) -> bool {
        matches!(self, Self::NormalizedAndRaw)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RequestContentLoggingResponse {
    pub mode: RequestContentLoggingMode,
    pub raw_retention_days: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct RequestContentLoggingRequest {
    pub mode: RequestContentLoggingMode,
    pub raw_retention_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(default)]
pub struct UsageRetentionSettings {
    pub metadata_retention_days: i32,
    pub content_retention_days: i32,
    pub raw_retention_days: i32,
    #[serde(default = "default_approval_retention_days")]
    pub approval_retention_days: i32,
    pub replay_enabled: bool,
    pub raw_backend: String,
}

fn default_approval_retention_days() -> i32 {
    90
}

impl Default for UsageRetentionSettings {
    fn default() -> Self {
        Self {
            metadata_retention_days: 90,
            content_retention_days: 3,
            raw_retention_days: 3,
            approval_retention_days: default_approval_retention_days(),
            replay_enabled: true,
            raw_backend: "object_store".to_string(),
        }
    }
}

impl UsageRetentionSettings {
    pub fn normalized(mut self) -> Self {
        self.metadata_retention_days = self.metadata_retention_days.max(1);
        self.content_retention_days = self.content_retention_days.max(1);
        self.raw_retention_days = self.raw_retention_days.max(1);
        self.approval_retention_days = self.approval_retention_days.max(1);
        // Raw payloads are always stored in the managed object store; the
        // legacy `postgres` backend (and any unknown value) normalizes to it.
        self.raw_backend = "object_store".to_string();
        self
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum RawObjectStoreSecretPatch {
    Keep,
    Clear,
    Replace { value: String },
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct RawObjectStoreSettingsRequest {
    pub backend: RawObjectStoreBackend,
    pub local_dir: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_prefix: String,
    pub s3_allow_http: bool,
    pub s3_access_key: Option<RawObjectStoreSecretPatch>,
    pub s3_secret_key: Option<RawObjectStoreSecretPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RelayIpPolicyResponse {
    pub allowed_cidrs: Vec<String>,
    pub trusted_proxy_cidrs: Vec<String>,
}

impl From<RelayIpPolicy> for RelayIpPolicyResponse {
    fn from(value: RelayIpPolicy) -> Self {
        Self {
            allowed_cidrs: value.allowed_cidrs,
            trusted_proxy_cidrs: value.trusted_proxy_cidrs,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::UsageRetentionSettings;

    #[test]
    fn usage_retention_normalizes_bounds_and_backend() {
        let normalized = UsageRetentionSettings {
            metadata_retention_days: 0,
            content_retention_days: -1,
            raw_retention_days: 0,
            approval_retention_days: 0,
            replay_enabled: true,
            raw_backend: " unsupported ".to_string(),
        }
        .normalized();

        assert_eq!(normalized.metadata_retention_days, 1);
        assert_eq!(normalized.content_retention_days, 1);
        assert_eq!(normalized.raw_retention_days, 1);
        assert_eq!(normalized.approval_retention_days, 1);
        // The legacy postgres backend no longer stores raw bodies.
        assert_eq!(normalized.raw_backend, "object_store");
    }

    #[test]
    fn legacy_postgres_raw_backend_normalizes_to_managed_store() {
        let normalized = UsageRetentionSettings {
            raw_backend: "postgres".to_string(),
            ..UsageRetentionSettings::default()
        }
        .normalized();

        assert_eq!(normalized.raw_backend, "object_store");
    }

    #[test]
    fn usage_retention_trims_supported_backend() {
        let normalized = UsageRetentionSettings {
            raw_backend: " object_store ".to_string(),
            ..UsageRetentionSettings::default()
        }
        .normalized();

        assert_eq!(normalized.raw_backend, "object_store");
    }

    #[test]
    fn legacy_usage_retention_defaults_approval_days() {
        let settings: UsageRetentionSettings = serde_json::from_value(json!({
            "metadata_retention_days": 90,
            "content_retention_days": 3,
            "raw_retention_days": 3,
            "replay_enabled": true,
            "raw_backend": "object_store"
        }))
        .expect("legacy usage retention should deserialize");

        assert_eq!(settings.approval_retention_days, 90);
    }
}
