use base64::{Engine, engine::general_purpose::STANDARD};
use cloudiful_notifier::{EmailChannel, EmailTlsMode, MessageEnvelope};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use uuid::Uuid;

use super::super::cache_alert_sender::{
    CACHE_ALERT_DELIVERY_TIMEOUT, CACHE_ALERT_TITLE, cache_alert_message,
};
use super::{
    deliver_cache_alert_email, describe_delivery_failure, email_channel, send_cache_alert_email,
};
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

fn plain_channel(port: u16) -> EmailChannel {
    EmailChannel {
        smtp_host: "127.0.0.1".to_string(),
        smtp_port: Some(port),
        tls_mode: EmailTlsMode::Plain,
        username: None,
        password: None,
        from: SENDER_ADDRESS.to_string(),
        to: vec![RECIPIENT_ADDRESS.to_string()],
        reply_to: None,
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

#[tokio::test]
async fn local_stub_receives_title_recipients_and_metadata_only_body() {
    let (port, received) = fake_smtp_server().await;
    let message = cache_alert_message(&settings(), &conversation());

    deliver_cache_alert_email(
        &reqwest::Client::new(),
        &plain_channel(port),
        &message,
        Duration::from_secs(5),
    )
    .await
    .expect("the local SMTP stub accepts the alert");

    let raw = String::from_utf8_lossy(&received.await.expect("captured message")).to_string();
    assert!(raw.to_ascii_lowercase().contains("subject:"));
    assert!(raw.contains(&STANDARD.encode(CACHE_ALERT_TITLE)), "{raw}");
    assert!(raw.contains(SENDER_ADDRESS), "{raw}");
    assert!(raw.contains(RECIPIENT_ADDRESS), "{raw}");
    assert!(raw.contains(&Uuid::nil().to_string()), "{raw}");
    assert!(raw.contains("model: gpt-test"), "{raw}");
    assert!(raw.contains("cache_rate: 8.0%"), "{raw}");
    assert!(raw.contains("threshold: 20.0%"), "{raw}");
    assert!(!raw.contains(SMTP_PASSWORD), "{raw}");
}

#[tokio::test]
async fn transport_failure_hides_credentials_host_and_addresses() {
    let error = deliver_cache_alert_email(
        &reqwest::Client::new(),
        &EmailChannel {
            smtp_port: Some(1),
            username: Some(SMTP_USERNAME.to_string()),
            password: Some(SMTP_PASSWORD.to_string()),
            ..plain_channel(1)
        },
        &MessageEnvelope::new("body").with_title(CACHE_ALERT_TITLE),
        Duration::from_secs(5),
    )
    .await
    .expect_err("an unreachable relay must fail");

    let text = error.to_string();
    assert!(text.contains("smtp delivery failed"), "{text}");
    for marker in [
        SMTP_PASSWORD,
        SMTP_USERNAME,
        SENDER_ADDRESS,
        RECIPIENT_ADDRESS,
    ] {
        assert!(!text.contains(marker), "{text}");
    }
    assert!(!text.contains("127.0.0.1"), "{text}");
}

#[tokio::test]
async fn notifier_message_errors_do_not_echo_the_addresses() {
    let error = deliver_cache_alert_email(
        &reqwest::Client::new(),
        &EmailChannel {
            to: Vec::new(),
            ..plain_channel(1)
        },
        &MessageEnvelope::new("body").with_title(CACHE_ALERT_TITLE),
        Duration::from_secs(1),
    )
    .await
    .expect_err("an email without recipients must fail");

    let text = error.to_string();
    assert!(text.contains("invalid smtp alert message"), "{text}");
    assert!(!text.contains(SMTP_PASSWORD), "{text}");
    assert!(!text.contains(SENDER_ADDRESS), "{text}");
}

#[tokio::test]
async fn delivery_timeout_bounds_a_hung_relay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(stream);
    });

    let error = deliver_cache_alert_email(
        &reqwest::Client::new(),
        &plain_channel(port),
        &MessageEnvelope::new("body").with_title(CACHE_ALERT_TITLE),
        Duration::from_millis(100),
    )
    .await
    .expect_err("a hung relay must time out");

    let text = error.to_string();
    assert!(text.contains("timed out"), "{text}");
    assert!(!text.contains(SMTP_PASSWORD), "{text}");
    assert!(!text.contains(RECIPIENT_ADDRESS), "{text}");
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

/// Minimal SMTP conversation: greeting, EHLO, envelope, DATA capture, and
/// QUIT. It answers in plaintext so the test can observe the real message
/// without a TLS fixture; production settings cannot select this transport.
async fn fake_smtp_server() -> (u16, tokio::sync::oneshot::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        let mut message = Vec::new();
        writer.write_all(b"220 fake ESMTP ready\r\n").await.unwrap();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                break;
            }
            let command = line.trim_end().to_ascii_uppercase();
            if command.starts_with("EHLO") || command.starts_with("HELO") {
                writer
                    .write_all(b"250-fake\r\n250 AUTH PLAIN LOGIN\r\n")
                    .await
                    .unwrap();
            } else if command.starts_with("MAIL FROM") || command.starts_with("RCPT TO") {
                writer.write_all(b"250 2.1.0 Ok\r\n").await.unwrap();
            } else if command == "DATA" {
                writer
                    .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                    .await
                    .unwrap();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    if line == ".\r\n" || line == ".\n" {
                        break;
                    }
                    message.extend_from_slice(line.as_bytes());
                }
                writer.write_all(b"250 2.0.0 Ok: queued\r\n").await.unwrap();
            } else if command.starts_with("QUIT") {
                writer.write_all(b"221 2.0.0 Bye\r\n").await.unwrap();
                break;
            } else {
                writer.write_all(b"250 2.0.0 Ok\r\n").await.unwrap();
            }
        }
        let _ = sender.send(message);
    });
    (port, receiver)
}
