use super::*;
use crate::{db::EndpointProvider, worker_admin_types::*};

#[tokio::test]
async fn every_supported_provider_blocks_known_zero_and_accepts_positive() {
    for provider in [
        EndpointProvider::Minimax,
        EndpointProvider::CommandCode,
        EndpointProvider::OpencodeGo,
        EndpointProvider::OpenRouter,
        EndpointProvider::Glm,
        EndpointProvider::DeepSeek,
    ] {
        for remaining in [0.0, 50.0] {
            let cache = TokenPlanQuotaCache::default();
            let endpoint = Uuid::new_v4();
            let key = Uuid::new_v4();
            let mut usage = opencode_go_usage(&[(key, "k", remaining)]);
            usage.provider = provider;
            let row = &mut usage.keys[0];
            row.opencodego_weekly = None;
            let reset = Some(chrono::Utc::now() + chrono::Duration::seconds(1));
            match provider {
                EndpointProvider::Minimax => {
                    row.model_remains = vec![model_scope::model("general", remaining, None)]
                }
                EndpointProvider::CommandCode => {
                    row.five_hour = Some(CommandCodeWindowUsage {
                        used: 100.0 - remaining,
                        cap: 100.0,
                        used_percent: Some(100.0 - remaining),
                        remaining_percent: Some(remaining),
                        reset_at: reset,
                    })
                }
                EndpointProvider::OpencodeGo => {
                    row.opencodego_weekly = Some(OpencodeGoWindowUsage {
                        status: None,
                        percent: Some(100.0 - remaining),
                        resets_at: reset,
                    })
                }
                EndpointProvider::OpenRouter => {
                    row.openrouter_balance = Some(OpenRouterBalance {
                        limit: Some(100.0),
                        limit_remaining: Some(remaining),
                        limit_reset: None,
                        is_free_tier: false,
                        total_credits: None,
                        total_usage: None,
                    })
                }
                EndpointProvider::Glm => {
                    row.glm_five_hour = Some(GlmWindowUsage {
                        limit: 100.0,
                        current_value: 100.0 - remaining,
                        remaining,
                        percentage: Some(100.0 - remaining),
                        next_reset_at: reset,
                    })
                }
                EndpointProvider::DeepSeek => {
                    row.deepseek_balance = Some(DeepSeekBalance {
                        is_available: remaining > 0.0,
                        balances: Vec::new(),
                    })
                }
                _ => unreachable!(),
            }
            cache.store_for_test(endpoint, usage).await;
            let result = assess(Some(&cache), endpoint, Some(key), text());
            if remaining == 0.0 {
                assert_eq!(result, Eligibility::Exhausted, "{provider:?}");
            } else {
                assert!(
                    result.weight().is_some_and(|weight| weight > 0.0),
                    "{provider:?}"
                );
            }
        }
    }
}
