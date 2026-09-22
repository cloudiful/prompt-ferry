use super::{
    CACHE_ALERT_DELIVERY_TIMEOUT, CACHE_ALERT_TITLE, cache_alert_message, deliver_cache_alert,
    describe_delivery_failure, send_cache_alert,
};
use crate::{db::LowCacheConversation, worker_admin_types::CacheAlertSettings};
use axum::{Json, Router, http::StatusCode, routing::post};
use chrono::{TimeZone, Utc};
use cloudiful_notifier::{DingtalkChannel, DingtalkMessageType, MessageEnvelope, NotifierError};
use std::time::Duration;
use uuid::Uuid;

/// Credential-shaped marker that must never reach a log line.
const WEBHOOK_SECRET: &str = "SEC-webhook-access-token";

#[test]
fn production_delivery_timeout_is_ten_seconds() {
    // The shared worker client only bounds connection establishment, so the
    // alert path itself owns a 10s ceiling.
    const { assert!(CACHE_ALERT_DELIVERY_TIMEOUT.as_secs() == 10) };
}

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
    }
}

fn channel(webhook_url: &str) -> DingtalkChannel {
    DingtalkChannel {
        webhook_url: webhook_url.to_string(),
        secret: Some("SEC-robot-secret".to_string()),
        keywords: Vec::new(),
        message_type: DingtalkMessageType::Text,
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

#[tokio::test]
async fn transport_failure_error_hides_the_webhook_credential() {
    // Connection refused: reqwest renders ` for url (http://…access_token=…)`
    // in its Display impl, so the sanitizer must drop the whole message.
    let unreachable = format!("http://127.0.0.1:1/robot/send?access_token={WEBHOOK_SECRET}");
    let error = deliver_cache_alert(
        &reqwest::Client::new(),
        &channel(&unreachable),
        &MessageEnvelope::new("body"),
        Duration::from_secs(5),
    )
    .await
    .expect_err("an unreachable webhook must fail");

    let text = error.to_string();
    assert!(!text.contains(WEBHOOK_SECRET), "{text}");
    assert!(!text.contains("access_token"), "{text}");
    assert!(text.contains("dingtalk request failed"), "{text}");
}

#[tokio::test]
async fn provider_rejection_reports_the_code_without_the_webhook_url() {
    let app = Router::new().route(
        "/robot/send",
        post(|| async {
            (
                StatusCode::OK,
                Json(serde_json::json!({ "errcode": 310000, "errmsg": "sign not match" })),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let webhook_url = format!("http://{address}/robot/send?access_token={WEBHOOK_SECRET}");

    let error = deliver_cache_alert(
        &reqwest::Client::new(),
        &channel(&webhook_url),
        &MessageEnvelope::new("body"),
        Duration::from_secs(5),
    )
    .await
    .expect_err("a rejected alert must fail");

    let text = error.to_string();
    assert!(text.contains("310000"), "{text}");
    assert!(text.contains("sign not match"), "{text}");
    assert!(!text.contains(WEBHOOK_SECRET), "{text}");
}

#[tokio::test]
async fn delivery_timeout_bounds_a_hung_endpoint() {
    async fn hang() -> StatusCode {
        tokio::time::sleep(Duration::from_secs(30)).await;
        StatusCode::OK
    }

    let app = Router::new().route("/robot/send", post(hang));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let webhook_url = format!("http://{address}/robot/send?access_token={WEBHOOK_SECRET}");

    let error = deliver_cache_alert(
        &reqwest::Client::new(),
        &channel(&webhook_url),
        &MessageEnvelope::new("body"),
        Duration::from_millis(100),
    )
    .await
    .expect_err("a hung endpoint must time out");

    let text = error.to_string();
    assert!(text.contains("timed out"), "{text}");
    assert!(!text.contains(WEBHOOK_SECRET), "{text}");
}
