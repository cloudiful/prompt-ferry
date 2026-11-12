use super::{CACHE_ALERT_TITLE, cache_alert_message, describe_delivery_failure, send_cache_alert};
use crate::{db::LowCacheConversation, worker_admin_types::CacheAlertSettings};
use chrono::{TimeZone, Utc};
use cloudiful_notifier::NotifierError;
use uuid::Uuid;

/// Credential-shaped marker that must never reach a log line.
const WEBHOOK_SECRET: &str = "SEC-webhook-access-token";

fn conversation() -> LowCacheConversation {
    LowCacheConversation {
        conversation_id: Uuid::nil(),
        model: Some("gpt-test".to_string()),
        turns: 6,
        cache_rate: Some(0.08),
        window_start: Utc.with_ymd_and_hms(2026, 9, 21, 10, 0, 0).unwrap(),
        window_end: Utc.with_ymd_and_hms(2026, 9, 21, 10, 30, 0).unwrap(),
    }
}

fn settings() -> CacheAlertSettings {
    CacheAlertSettings {
        enabled: true,
        window_minutes: 30,
        min_turns: 5,
        threshold: 0.2,
        cooldown_minutes: 60,
        dingtalk_webhook_url: format!(
            "https://oapi.dingtalk.com/robot/send?access_token={WEBHOOK_SECRET}"
        ),
        dingtalk_secret: String::new(),
        smtp: Default::default(),
    }
}

#[tokio::test]
async fn disabled_alerting_skips_delivery() {
    // A disabled policy must not attempt delivery: the invalid webhook
    // would fail inside the channel if any request were made.
    let disabled = CacheAlertSettings {
        enabled: false,
        dingtalk_webhook_url: "not-a-url".to_string(),
        ..settings()
    };

    send_cache_alert(&reqwest::Client::new(), &disabled, &conversation())
        .await
        .expect("disabled alerting must not send anything");
}

#[tokio::test]
async fn missing_webhook_skips_delivery() {
    let without_webhook = CacheAlertSettings {
        dingtalk_webhook_url: "   ".to_string(),
        ..settings()
    };

    send_cache_alert(&reqwest::Client::new(), &without_webhook, &conversation())
        .await
        .expect("an unconfigured webhook must not send anything");
}

#[test]
fn message_carries_only_conversation_metadata() {
    let message = cache_alert_message(&settings(), &conversation());

    assert_eq!(message.title.as_deref(), Some(CACHE_ALERT_TITLE));
    assert!(message.body.contains(&Uuid::nil().to_string()));
    assert!(message.body.contains("model: gpt-test"));
    assert!(message.body.contains("window: 30m ("));
    assert!(message.body.contains("turns: 6"));
    assert!(message.body.contains("cache_rate: 8.0%"));
    assert!(message.body.contains("threshold: 20.0%"));
}

#[test]
fn message_marks_a_missing_model_instead_of_dropping_the_field() {
    let message = cache_alert_message(
        &settings(),
        &LowCacheConversation {
            model: None,
            ..conversation()
        },
    );

    assert!(message.body.contains("model: -"));
}

#[test]
fn invalid_url_failure_hides_the_webhook_credential() {
    // `InvalidUrl` renders the URL verbatim, which carries access_token.
    let described = describe_delivery_failure(&NotifierError::InvalidUrl {
        url: settings().dingtalk_webhook_url,
        message: "relative URL without a base".to_string(),
    });

    assert!(!described.contains(WEBHOOK_SECRET));
    assert!(described.contains("invalid dingtalk webhook URL"));
}

#[test]
fn transport_failure_error_hides_the_webhook_credential() {
    // A transport error can embed the request URL, so the sanitizer must drop
    // the whole message instead of forwarding it.
    let described = describe_delivery_failure(&NotifierError::Transport {
        provider: "dingtalk",
        message: format!(
            "connection refused for http://127.0.0.1:1/robot/send?access_token={WEBHOOK_SECRET}"
        ),
    });

    assert!(!described.contains(WEBHOOK_SECRET), "{described}");
    assert!(!described.contains("access_token"), "{described}");
    assert_eq!(described, "dingtalk request failed");
}
