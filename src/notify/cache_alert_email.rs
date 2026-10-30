use std::time::Duration;

use cloudiful_notifier::{EmailChannel, EmailTlsMode, MessageEnvelope, Notifier, NotifierError};

use crate::{
    db::LowCacheConversation,
    worker_admin_types::{CacheAlertSettings, CacheAlertSmtpTls},
};

use super::cache_alert_sender::{CACHE_ALERT_DELIVERY_TIMEOUT, cache_alert_message};

/// Deliver one low-cache conversation alert through SMTP.
///
/// Returns `Ok(())` without any connection when alerting or the SMTP channel is
/// disabled, so a half-configured policy never fails a tick. The message reuses
/// the DingTalk body and therefore carries conversation metadata only:
/// conversation id, model, window, turns, cache rate, and threshold.
pub async fn send_cache_alert_email(
    client: &reqwest::Client,
    settings: &CacheAlertSettings,
    conversation: &LowCacheConversation,
) -> anyhow::Result<()> {
    if !settings.enabled || !settings.smtp.enabled {
        return Ok(());
    }
    let channel = email_channel(settings);
    let message = cache_alert_message(settings, conversation);
    deliver_cache_alert_email(client, &channel, &message, CACHE_ALERT_DELIVERY_TIMEOUT).await
}

/// Build the notifier channel from the stored policy. Only encrypted transport
/// modes exist in the settings contract, so an external plaintext SMTP relay is
/// not expressible.
fn email_channel(settings: &CacheAlertSettings) -> EmailChannel {
    EmailChannel {
        smtp_host: settings.smtp.host.trim().to_string(),
        smtp_port: Some(settings.smtp.port),
        tls_mode: match settings.smtp.tls {
            CacheAlertSmtpTls::ImplicitTls => EmailTlsMode::ImplicitTls,
            CacheAlertSmtpTls::StartTls => EmailTlsMode::StartTls,
        },
        username: optional_secret(&settings.smtp.username),
        password: optional_secret(&settings.smtp.password),
        from: settings.smtp.from.trim().to_string(),
        to: settings
            .smtp
            .to
            .iter()
            .map(|recipient| recipient.trim().to_string())
            .filter(|recipient| !recipient.is_empty())
            .collect(),
        reply_to: None,
    }
}

async fn deliver_cache_alert_email(
    client: &reqwest::Client,
    channel: &EmailChannel,
    message: &MessageEnvelope,
    timeout: Duration,
) -> anyhow::Result<()> {
    let notifier = Notifier::new(client.clone());
    let delivery = notifier.send(channel, message);
    match tokio::time::timeout(timeout, delivery).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(anyhow::anyhow!("{}", describe_delivery_failure(&error))),
        Err(_) => Err(anyhow::anyhow!(
            "smtp cache alert delivery timed out after {}s",
            timeout.as_secs_f32()
        )),
    }
}

/// Classify a notifier failure without echoing the SMTP host, username,
/// addresses, or password. The notifier wraps transport errors verbatim and
/// those displays can name the relay or mailbox, so only provider-neutral
/// categories and provider-owned status codes are safe to log.
fn describe_delivery_failure(error: &NotifierError) -> String {
    match error {
        NotifierError::HttpStatus { status, .. } => {
            format!("smtp request failed with HTTP status {status}")
        }
        NotifierError::ProviderRejected { code, .. } => {
            format!("smtp rejected the alert with code {code}")
        }
        NotifierError::InvalidMessage { .. } => "invalid smtp alert message".to_string(),
        NotifierError::UnsupportedUrlScheme { .. } => {
            "unsupported smtp endpoint scheme".to_string()
        }
        NotifierError::InvalidUrl { .. } => "invalid smtp endpoint".to_string(),
        NotifierError::InvalidHeaderName { .. }
        | NotifierError::InvalidHeaderValue { .. }
        | NotifierError::ReservedHeader { .. } => "invalid smtp request headers".to_string(),
        NotifierError::InvalidSecret { .. } => "invalid smtp credentials".to_string(),
        NotifierError::HttpRequest { .. } | NotifierError::Transport { .. } => {
            "smtp delivery failed".to_string()
        }
        NotifierError::ResponseDecode { .. } => "failed to decode the smtp response".to_string(),
    }
}

fn optional_secret(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
#[path = "cache_alert_email_tests.rs"]
mod tests;
