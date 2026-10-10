use anyhow::Result;
use uuid::Uuid;

use crate::{
    db::{ChatgptQuotaSnapshot, ProviderEndpoint},
    worker_admin::handlers::chatgpt_backend::{ChatgptQuota, ChatgptQuotaWindow},
    worker_admin_types::{
        SubscriptionQuotaObservation, SubscriptionWindowAvailability, SubscriptionWindowUsage,
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse,
    },
};

pub(crate) fn subscription_windows(quota: &ChatgptQuota) -> Vec<SubscriptionWindowUsage> {
    let mut windows = Vec::new();
    if let Some(window) = quota.primary.as_ref() {
        windows.push(subscription_window("primary", window));
    }
    if let Some(window) = quota.secondary.as_ref() {
        windows.push(subscription_window("secondary", window));
    }
    windows
}

fn subscription_window(
    source_window: &str,
    window: &ChatgptQuotaWindow,
) -> SubscriptionWindowUsage {
    let used_percent = window
        .used_percent
        .filter(|used| used.is_finite() && (0.0..=100.0).contains(used));
    SubscriptionWindowUsage {
        source_window: source_window.to_string(),
        window_seconds: window.limit_window_seconds,
        used_percent,
        remaining_percent: used_percent.map(|used| 100.0 - used),
        reset_at: window.reset_at,
        reset_after_seconds: window.reset_after_seconds,
        availability: if used_percent.is_some() {
            SubscriptionWindowAvailability::Known
        } else {
            SubscriptionWindowAvailability::Unknown
        },
    }
}

pub(crate) fn response_from_quota(
    endpoint: &ProviderEndpoint,
    quota: ChatgptQuota,
) -> TokenPlanUsageResponse {
    let windows = subscription_windows(&quota);
    let model_remains = if windows.is_empty() {
        Vec::new()
    } else {
        vec![TokenPlanModelUsage {
            model_name: quota
                .plan_type
                .clone()
                .unwrap_or_else(|| "chatgpt".to_string()),
            interval: None,
            weekly: None,
            windows: Some(windows),
            observation: None,
        }]
    };
    response(endpoint, quota.plan_type, model_remains)
}

pub(crate) fn response_from_snapshot(
    endpoint: &ProviderEndpoint,
    snapshot: &ChatgptQuotaSnapshot,
    observation: SubscriptionQuotaObservation,
) -> Result<TokenPlanUsageResponse> {
    let windows = serde_json::from_value(snapshot.windows.clone())?;
    let model = TokenPlanModelUsage {
        model_name: snapshot
            .plan_type
            .clone()
            .unwrap_or_else(|| "chatgpt".to_string()),
        interval: None,
        weekly: None,
        windows: Some(windows),
        observation: Some(observation),
    };
    Ok(response(endpoint, snapshot.plan_type.clone(), vec![model]))
}

fn response(
    endpoint: &ProviderEndpoint,
    plan_type: Option<String>,
    model_remains: Vec<TokenPlanModelUsage>,
) -> TokenPlanUsageResponse {
    let key_label = plan_type
        .clone()
        .map(|plan_type| format!("ChatGPT {plan_type}"))
        .unwrap_or_else(|| "ChatGPT subscription".to_string());
    TokenPlanUsageResponse {
        provider: endpoint.provider,
        provider_region: endpoint.provider_region,
        keys: vec![TokenPlanKeyUsage {
            key_id: endpoint
                .api_keys
                .first()
                .map(|key| key.key_id)
                .unwrap_or(Uuid::nil()),
            key_label,
            ok: true,
            status: None,
            error_code: None,
            error_message: None,
            model_remains,
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
        }],
        local_today_tokens: None,
    }
}
