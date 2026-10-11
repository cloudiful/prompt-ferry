use super::*;
use crate::worker_admin_types::{TokenPlanModelUsage, TokenPlanWindowUsage};

pub(super) fn model(name: &str, short: f64, long: Option<f64>) -> TokenPlanModelUsage {
    let window = |remaining| TokenPlanWindowUsage {
        status: None,
        remaining_percent: Some(remaining),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: None,
        remains_time_ms: None,
    };
    TokenPlanModelUsage {
        model_name: name.into(),
        interval: Some(window(short)),
        weekly: long.map(window),
        windows: None,
        observation: None,
    }
}

#[tokio::test]
async fn operation_general_and_exact_upstream_model_do_not_mix_budgets() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let key = Uuid::new_v4();
    let mut usage = opencode_go_usage(&[(key, "k", 100.0)]);
    usage.keys[0].opencodego_weekly = None;
    usage.keys[0].model_remains = vec![
        model("GeNeRaL", 100.0, Some(0.0)),
        model("video", 100.0, Some(100.0)),
    ];
    cache.store_for_test(endpoint, usage).await;
    for path in [
        "/v1/messages",
        "/v1/chat/completions",
        "/v1/responses",
        "/v1/responses/compact",
    ] {
        assert_eq!(
            assess(
                Some(&cache),
                endpoint,
                Some(key),
                QuotaContext { path, ..text() }
            ),
            Eligibility::Exhausted
        );
    }
    for path in ["/v1/realtime", "/v1/unknown"] {
        assert_eq!(
            assess(
                Some(&cache),
                endpoint,
                Some(key),
                QuotaContext { path, ..text() }
            ),
            Eligibility::Eligible(1.0)
        );
    }
    assert_eq!(
        assess(
            Some(&cache),
            endpoint,
            Some(key),
            text().effective(Some("VIDEO"))
        ),
        Eligibility::Eligible(100.0)
    );
    let mut only_video = opencode_go_usage(&[(key, "k", 100.0)]);
    only_video.keys[0].opencodego_weekly = None;
    only_video.keys[0].model_remains = vec![model("video", 0.0, None)];
    cache.store_for_test(endpoint, only_video).await;
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Eligible(1.0)
    );
    assert_eq!(
        assess(
            Some(&cache),
            endpoint,
            Some(key),
            text().effective(Some("video"))
        ),
        Eligibility::Exhausted
    );
}
