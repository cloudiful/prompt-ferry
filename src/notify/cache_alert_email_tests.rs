use cloudiful_notifier::EmailTlsMode;
use uuid::Uuid;

use super::super::cache_alert_sender::CACHE_ALERT_DELIVERY_TIMEOUT;
use super::{describe_delivery_failure, email_channel, send_cache_alert_email};
use crate::{
    db::LowCacheConversation,
    worker_admin_types::{CacheAlertSettings, CacheAlertSmtpSettings, CacheAlertSmtpTls},
};
use chrono::{TimeZone, Utc};
use cloudiful_notifier::NotifierError;

const SMTP_PASSWORD: &str = "SEC-smtp-password";
const SMTP_USERNAME: &str = "alert-user";
const SENDER_ADDRESS: &str = "alerts@example.com";
const RECIPIENT_ADDRESS: &str = "ops@example.com";

#[test]
fn production_delivery_timeout_is_ten_seconds() {
    // Both channels own the same 10s ceiling; the shared worker client only
    // bounds connection establishment.
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

fn smtp_settings() -> CacheAlertSmtpSettings {
    CacheAlertSmtpSettings {
        enabled: true,
        host: "smtp.example.com".to_string(),
        port: 587,
        tls: CacheAlertSmtpTls::StartTls,
        from: SENDER_ADDRESS.to_string(),
        to: vec![RECIPIENT_ADDRESS.to_string()],
        username: SMTP_USERNAME.to_string(),
        password: SMTP_PASSWORD.to_string(),
    }
}

fn settings() -> CacheAlertSettings {
    CacheAlertSettings {
        enabled: true,
        window_minutes: 30,
        min_turns: 5,
        threshold: 0.2,
        cooldown_minutes: 60,
        dingtalk_webhook_url: String::new(),
        dingtalk_secret: String::new(),
        smtp: smtp_settings(),
    }
}

#[test]
fn channel_uses_the_encrypted_transport_and_stored_recipients() {
    let channel = email_channel(&settings());

    assert_eq!(channel.smtp_host, "smtp.example.com");
    assert_eq!(channel.smtp_port, Some(587));
    assert_eq!(channel.tls_mode, EmailTlsMode::StartTls);
    assert_eq!(channel.username.as_deref(), Some(SMTP_USERNAME));
    assert_eq!(channel.password.as_deref(), Some(SMTP_PASSWORD));
    assert_eq!(channel.from, SENDER_ADDRESS);
    assert_eq!(channel.to, vec![RECIPIENT_ADDRESS.to_string()]);

    let implicit = email_channel(&CacheAlertSettings {
        smtp: CacheAlertSmtpSettings {
            tls: CacheAlertSmtpTls::ImplicitTls,
            ..smtp_settings()
        },
        ..settings()
    });
    assert_eq!(implicit.tls_mode, EmailTlsMode::ImplicitTls);
}

#[tokio::test]
async fn disabled_smtp_or_global_policy_skips_delivery() {
    // The invalid host would fail inside the channel if any connection were
    // attempted.
    let disabled = CacheAlertSettings {
        enabled: false,
        smtp: CacheAlertSmtpSettings {
            host: "not a host".to_string(),
            ..smtp_settings()
        },
        ..settings()
    };
    send_cache_alert_email(&reqwest::Client::new(), &disabled, &conversation())
        .await
        .expect("a globally disabled policy must not send anything");

    let channel_disabled = CacheAlertSettings {
        smtp: CacheAlertSmtpSettings {
            enabled: false,
            host: "not a host".to_string(),
            ..smtp_settings()
        },
        ..settings()
    };
    send_cache_alert_email(&reqwest::Client::new(), &channel_disabled, &conversation())
        .await
        .expect("a disabled SMTP channel must not send anything");
}

#[test]
fn transport_failure_hides_credentials_host_and_addresses() {
    let described = describe_delivery_failure(&NotifierError::Transport {
        provider: "smtp",
        message: format!(
            "connection to 127.0.0.1 as {SMTP_USERNAME} <{SENDER_ADDRESS}> to {RECIPIENT_ADDRESS} with {SMTP_PASSWORD} failed"
        ),
    });

    assert_eq!(described, "smtp delivery failed");
    for marker in [
        SMTP_PASSWORD,
        SMTP_USERNAME,
        SENDER_ADDRESS,
        RECIPIENT_ADDRESS,
        "127.0.0.1",
    ] {
        assert!(!described.contains(marker), "{described}");
    }
}

#[test]
fn notifier_message_errors_do_not_echo_the_addresses() {
    let described = describe_delivery_failure(&NotifierError::InvalidMessage {
        provider: "smtp",
        message: format!(
            "email to {RECIPIENT_ADDRESS} from {SENDER_ADDRESS} has no recipients and password {SMTP_PASSWORD}"
        ),
    });

    assert!(
        described.contains("invalid smtp alert message"),
        "{described}"
    );
    assert!(!described.contains(SMTP_PASSWORD), "{described}");
    assert!(!described.contains(SENDER_ADDRESS), "{described}");
}

#[test]
fn failure_classification_is_provider_neutral() {
    let described = describe_delivery_failure(&NotifierError::Transport {
        provider: "email",
        message: format!(
            "connection to smtp.example.com as {SMTP_USERNAME} <{SENDER_ADDRESS}> failed with {SMTP_PASSWORD}"
        ),
    });

    assert_eq!(described, "smtp delivery failed");
}
