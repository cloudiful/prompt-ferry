use std::time::Instant;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use uuid::Uuid;

use crate::{
    db::EndpointProvider,
    worker_admin::{
        quota_feedback::QuotaFeedbackScope,
        quota_reset::{self, ResetState},
    },
    worker_admin_types::{
        SubscriptionQuotaObservation, TokenPlanKeyUsage, TokenPlanQuotaState, TokenPlanQuotaStatus,
        TokenPlanUsageResponse,
    },
};

use super::{token_plan_cache::TokenPlanQuotaCache, token_plan_weight};

const SUBSCRIPTION_FRESHNESS: ChronoDuration = ChronoDuration::minutes(5);

#[cfg(test)]
mod reset_tests;
#[cfg(test)]
mod tests;

pub(crate) fn decorate_usage(
    cache: &TokenPlanQuotaCache,
    endpoint_id: Uuid,
    mut usage: TokenPlanUsageResponse,
) -> TokenPlanUsageResponse {
    let now = Utc::now();
    let instant = Instant::now();
    for key in &mut usage.keys {
        key.quota_status = Some(status_for_key(
            cache,
            endpoint_id,
            usage.provider,
            key,
            now,
            instant,
        ));
    }
    usage
}

fn status_for_key(
    cache: &TokenPlanQuotaCache,
    endpoint_id: Uuid,
    provider: EndpointProvider,
    key: &TokenPlanKeyUsage,
    now: DateTime<Utc>,
    instant: Instant,
) -> TokenPlanQuotaStatus {
    let subscription = (provider == EndpointProvider::OpenAi)
        .then(|| subscription_observation(key))
        .flatten();
    let cached = (provider != EndpointProvider::OpenAi)
        .then(|| cache.quota_observation_now(endpoint_id, key.key_id))
        .flatten();
    let observed_at = subscription
        .and_then(|observation| observation.observed_at)
        .or_else(|| cached.as_ref()?.observed_at);
    let refresh_failed = subscription
        .is_some_and(|observation| observation.last_error_code.is_some())
        || cached
            .as_ref()
            .is_some_and(|observation| observation.refresh_failed);
    let scope = QuotaFeedbackScope::new(
        endpoint_id,
        (provider != EndpointProvider::OpenAi).then_some(key.key_id),
    );
    let cooldown = cache.feedback.remaining_cooldown(scope, instant);
    let (state, cooldown_until) = if let Some(remaining) = cooldown {
        (
            TokenPlanQuotaState::Cooling,
            ChronoDuration::from_std(remaining)
                .ok()
                .and_then(|remaining| now.checked_add_signed(remaining)),
        )
    } else {
        let state = if provider == EndpointProvider::OpenAi {
            subscription
                .map(|observation| subscription_state(cache, endpoint_id, key, observation, now))
                .unwrap_or(TokenPlanQuotaState::Unknown)
        } else {
            cached
                .as_ref()
                .and_then(|observation| observation.key.as_ref())
                .map(|fresh_key| provider_state(provider, fresh_key, now))
                .unwrap_or(TokenPlanQuotaState::Unknown)
        };
        (state, None)
    };
    TokenPlanQuotaStatus {
        state,
        observed_at,
        cooldown_until,
        refresh_failed,
        scope: "text_inference".into(),
    }
}

fn subscription_observation(key: &TokenPlanKeyUsage) -> Option<&SubscriptionQuotaObservation> {
    key.model_remains
        .iter()
        .find(|model| model.windows.is_some())
        .and_then(|model| model.observation.as_ref())
}

fn subscription_state(
    cache: &TokenPlanQuotaCache,
    endpoint_id: Uuid,
    key: &TokenPlanKeyUsage,
    observation: &SubscriptionQuotaObservation,
    now: DateTime<Utc>,
) -> TokenPlanQuotaState {
    let Some(observed_at) = observation.observed_at else {
        return TokenPlanQuotaState::Unknown;
    };
    if observation.stale
        || observation.last_error_code.is_some()
        || now < observed_at
        || now.signed_duration_since(observed_at) > SUBSCRIPTION_FRESHNESS
    {
        return TokenPlanQuotaState::Unknown;
    }
    if cache.chatgpt_subscription_exhausted_now(endpoint_id) {
        return TokenPlanQuotaState::Exhausted;
    }
    let windows = key
        .model_remains
        .iter()
        .filter_map(|model| model.windows.as_deref())
        .flatten()
        .collect::<Vec<_>>();
    quota_reset::subscription_windows_state(&windows, observed_at, now)
}

fn provider_state(
    provider: EndpointProvider,
    key: &TokenPlanKeyUsage,
    now: DateTime<Utc>,
) -> TokenPlanQuotaState {
    match provider {
        EndpointProvider::Minimax => {
            let Some(model) = key
                .model_remains
                .iter()
                .find(|model| model.model_name.eq_ignore_ascii_case("general"))
            else {
                return TokenPlanQuotaState::Unknown;
            };
            window_state(
                [model.interval.as_ref(), model.weekly.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|window| {
                        (
                            window.remaining_percent,
                            quota_reset::reset_state_from_remaining(
                                window.end_at,
                                window.remains_time_ms,
                                now,
                            ),
                        )
                    }),
            )
        }
        EndpointProvider::CommandCode => {
            if let Some(balance) = key.balances.as_ref() {
                if balance.remaining_credits.is_finite() && balance.remaining_credits <= 0.0 {
                    return TokenPlanQuotaState::Exhausted;
                }
                if key.five_hour.is_none() && key.weekly.is_none() {
                    return if balance.remaining_credits.is_finite() {
                        TokenPlanQuotaState::Available
                    } else {
                        TokenPlanQuotaState::Unknown
                    };
                }
            }
            window_state(
                [key.five_hour.as_ref(), key.weekly.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|window| {
                        (
                            window.remaining_percent,
                            quota_reset::reset_state(window.reset_at, None, now),
                        )
                    }),
            )
        }
        EndpointProvider::OpencodeGo => window_state(
            [
                key.opencodego_rolling.as_ref(),
                key.opencodego_weekly.as_ref(),
                key.opencodego_monthly.as_ref(),
            ]
            .into_iter()
            .flatten()
            .map(|window| {
                (
                    window.percent.map(|used| 100.0 - used),
                    quota_reset::reset_state(window.resets_at, None, now),
                )
            }),
        ),
        EndpointProvider::OpenRouter => match token_plan_weight::provider_remaining_percent(key) {
            Some(remaining) if remaining.is_finite() && remaining <= 0.0 => {
                TokenPlanQuotaState::Exhausted
            }
            Some(remaining) if remaining.is_finite() => TokenPlanQuotaState::Available,
            _ => TokenPlanQuotaState::Unknown,
        },
        EndpointProvider::Glm => window_state(
            [key.glm_five_hour.as_ref(), key.glm_weekly.as_ref()]
                .into_iter()
                .flatten()
                .map(|window| {
                    (
                        window.percentage.map(|used| 100.0 - used),
                        quota_reset::reset_state(window.next_reset_at, None, now),
                    )
                }),
        ),
        EndpointProvider::DeepSeek => match key.deepseek_balance.as_ref() {
            Some(balance) if balance.is_available => TokenPlanQuotaState::Available,
            Some(_) => TokenPlanQuotaState::Exhausted,
            None => TokenPlanQuotaState::Unknown,
        },
        EndpointProvider::Generic | EndpointProvider::OpenAi => TokenPlanQuotaState::Unknown,
    }
}

fn window_state(windows: impl Iterator<Item = (Option<f64>, ResetState)>) -> TokenPlanQuotaState {
    let windows = windows.collect::<Vec<_>>();
    if windows.is_empty() {
        return TokenPlanQuotaState::Unknown;
    }
    let mut exhausted = false;
    let mut unknown = false;
    let mut positive = false;
    for (remaining, reset) in windows {
        match remaining.filter(|value| value.is_finite()) {
            Some(value) if value <= 0.0 => {
                if reset == ResetState::Passed {
                    unknown = true;
                } else {
                    exhausted = true;
                }
            }
            Some(value) if value <= 100.0 => positive = true,
            _ => unknown = true,
        }
    }
    if exhausted {
        TokenPlanQuotaState::Exhausted
    } else if unknown || !positive {
        TokenPlanQuotaState::Unknown
    } else {
        TokenPlanQuotaState::Available
    }
}
