use std::collections::BTreeMap;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFailurePolicy {
    FailOpen,
    #[default]
    FailClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, ToSchema)]
pub struct LlmReviewWebhookSettings {
    pub enabled: bool,
    pub url: String,
    pub bearer_token: String,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct LlmReviewSettings {
    pub enabled: bool,
    pub review_base_url: String,
    pub review_api_key: String,
    pub review_model: String,
    pub review_timeout_ms: u64,
    pub failure_policy: ReviewFailurePolicy,
    pub pending_ttl_seconds: u64,
    pub custom_policy_text: String,
    #[serde(default)]
    pub webhook: LlmReviewWebhookSettings,
}

impl Default for LlmReviewSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            review_base_url: String::new(),
            review_api_key: String::new(),
            review_model: String::new(),
            review_timeout_ms: 3_000,
            failure_policy: ReviewFailurePolicy::FailClosed,
            pending_ttl_seconds: 300,
            custom_policy_text: String::new(),
            webhook: LlmReviewWebhookSettings::default(),
        }
    }
}

impl LlmReviewSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.review_base_url.trim().is_empty() {
            return Err(anyhow!(
                "review_base_url is required when LLM review is enabled"
            ));
        }
        if self
            .review_base_url
            .trim()
            .trim_end_matches('/')
            .ends_with("/v1")
        {
            return Err(anyhow!("review_base_url must not include /v1"));
        }
        if self.review_model.trim().is_empty() {
            return Err(anyhow!(
                "review_model is required when LLM review is enabled"
            ));
        }
        if self.review_timeout_ms < 100 {
            return Err(anyhow!("review_timeout_ms must be at least 100"));
        }
        if self.pending_ttl_seconds == 0 {
            return Err(anyhow!("pending_ttl_seconds must be at least 1"));
        }
        if self.webhook.enabled && self.webhook.url.trim().is_empty() {
            return Err(anyhow!("webhook.url is required when webhook is enabled"));
        }
        for (name, value) in &self.webhook.extra_headers {
            if name.trim().is_empty() {
                return Err(anyhow!(
                    "webhook.extra_headers must not contain empty header names"
                ));
            }
            if value.contains('\n') || value.contains('\r') {
                return Err(anyhow!("webhook.extra_headers values must be single-line"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Allow,
    Flag,
    Error,
    Timeout,
}

impl ReviewDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Flag => "flag",
            Self::Error => "error",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Aborted,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Aborted => "aborted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResult {
    pub decision: ReviewDecision,
    pub reason: String,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewFailure {
    Timeout,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalResolution {
    Approved,
    Rejected,
    Expired,
    Interrupted,
}

#[derive(Debug)]
pub struct ApprovalWaiter {
    pub receiver: oneshot::Receiver<ApprovalResolution>,
}
