//! Shared test-only builders for the routing fixture families.
//!
//! Only the byte-for-byte equivalent fixture bodies live here. Each test
//! family keeps its own domain-specific quota and route builders.

use crate::db;
use crate::worker_admin_types::{
    CommandCodeWindowUsage, OpenRouterBalance, OpenRouterSpend, TokenPlanKeyUsage,
    TokenPlanWindowUsage,
};
use uuid::Uuid;

pub(super) fn endpoint_key(
    endpoint_id: Uuid,
    key_id: Uuid,
    label: &str,
    pos: i32,
) -> db::EndpointApiKey {
    db::EndpointApiKey {
        key_id,
        endpoint_id,
        key_label: label.to_string(),
        api_key: format!("{label}-key"),
        position: pos,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

pub(super) fn openrouter_key(
    key_id: Uuid,
    label: &str,
    limit: Option<f64>,
    remaining: Option<f64>,
    spend: bool,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        quota_status: None,
        model_remains: Vec::new(),
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: Some(OpenRouterBalance {
            limit,
            limit_remaining: remaining,
            limit_reset: None,
            is_free_tier: false,
            total_credits: None,
            total_usage: None,
        }),
        openrouter_spend: spend.then_some(OpenRouterSpend {
            usage: 1.0,
            daily: 0.5,
            weekly: 0.75,
            monthly: 1.0,
        }),
        glm_five_hour: None,
        glm_weekly: None,
        deepseek_balance: None,
        quota_windows: None,
    }
}

pub(super) fn window(remaining_percent: f64) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
        status: Some(1),
        remaining_percent: Some(remaining_percent),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: None,
        remains_time_ms: None,
    }
}

pub(super) fn command_code_window(remaining_percent: f64) -> CommandCodeWindowUsage {
    CommandCodeWindowUsage {
        used: 10.0 - remaining_percent / 10.0,
        cap: 10.0,
        used_percent: Some(100.0 - remaining_percent),
        remaining_percent: Some(remaining_percent),
        reset_at: None,
    }
}
