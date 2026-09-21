use std::time::Duration;

use cloudiful_notifier::{
    DingtalkChannel, DingtalkMessageType, MessageEnvelope, Notifier, NotifierError,
};

use crate::{db::LowCacheConversation, worker_admin_types::CacheAlertSettings};

/// DingTalk text robots deliver `title + "\n" + body`; the title doubles as
/// the robot keyword surface.
const CACHE_ALERT_TITLE: &str = "缓存率异常告警";

/// Hard ceiling for one alert delivery. The shared worker HTTP client only
/// bounds connection establishment, so a hung DingTalk endpoint would
/// otherwise stall the alert tick and hold its single-flight lease.
const CACHE_ALERT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Deliver one low-cache conversation alert through the DingTalk robot.
///
/// Returns `Ok(())` without any HTTP call when alerting is disabled or the
/// webhook is not configured, so a half-configured policy never fails a tick.
/// The message carries conversation metadata only: conversation id, model,
/// window, turns, cache rate, and threshold.
pub async fn send_cache_alert(
    client: &reqwest::Client,
    settings: &CacheAlertSettings,
    conversation: &LowCacheConversation,
) -> anyhow::Result<()> {
    if !settings.enabled {
        return Ok(());
    }
    let webhook_url = settings.dingtalk_webhook_url.trim();
    if webhook_url.is_empty() {
        return Ok(());
    }
    let channel = DingtalkChannel {
        webhook_url: webhook_url.to_string(),
        secret: optional_secret(&settings.dingtalk_secret),
        keywords: Vec::new(),
        message_type: DingtalkMessageType::Text,
    };
    let message = cache_alert_message(settings, conversation);
    deliver_cache_alert(client, &channel, &message, CACHE_ALERT_DELIVERY_TIMEOUT).await
}

async fn deliver_cache_alert(
    client: &reqwest::Client,
    channel: &DingtalkChannel,
    message: &MessageEnvelope,
    timeout: Duration,
) -> anyhow::Result<()> {
    let notifier = Notifier::new(client.clone());
    let delivery = notifier.send(channel, message);
    match tokio::time::timeout(timeout, delivery).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(anyhow::anyhow!("{}", describe_delivery_failure(&error))),
        Err(_) => Err(anyhow::anyhow!(
            "dingtalk cache alert delivery timed out after {}s",
            timeout.as_secs_f32()
        )),
    }
}

/// Classify a notifier failure without echoing anything that can carry the
/// webhook credential. The DingTalk robot URL embeds `access_token` and gains
/// `timestamp`/`sign` when a secret is configured; it appears verbatim in
/// `InvalidUrl` and inside `reqwest::Error` displays for transport failures,
/// so only provider-neutral categories and provider-owned status codes are
/// safe to log.
fn describe_delivery_failure(error: &NotifierError) -> String {
    match error {
        NotifierError::HttpStatus { status, .. } => {
            format!("dingtalk request failed with HTTP status {status}")
        }
        NotifierError::ProviderRejected { code, message, .. } => {
            format!("dingtalk rejected the alert with code {code}: {message}")
        }
        NotifierError::InvalidMessage { message, .. } => {
            format!("invalid dingtalk alert message: {message}")
        }
        NotifierError::UnsupportedUrlScheme { scheme } => {
            format!("unsupported dingtalk webhook URL scheme `{scheme}`")
        }
        NotifierError::InvalidUrl { .. } => "invalid dingtalk webhook URL".to_string(),
        NotifierError::InvalidHeaderName { .. }
        | NotifierError::InvalidHeaderValue { .. }
        | NotifierError::ReservedHeader { .. } => "invalid dingtalk request headers".to_string(),
        NotifierError::InvalidSecret { .. } => "invalid dingtalk robot secret".to_string(),
        NotifierError::HttpRequest { .. } | NotifierError::Transport { .. } => {
            "dingtalk request failed".to_string()
        }
        NotifierError::ResponseDecode { .. } => {
            "failed to decode the dingtalk response".to_string()
        }
    }
}

fn optional_secret(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn cache_alert_message(
    settings: &CacheAlertSettings,
    conversation: &LowCacheConversation,
) -> MessageEnvelope {
    let cache_rate = conversation.cache_rate.unwrap_or_default() * 100.0;
    let threshold = settings.threshold * 100.0;
    let body = format!(
        "conversation_id: {}\nmodel: {}\nwindow: {}m ({} ~ {})\nturns: {}\ncache_rate: {:.1}%\nthreshold: {:.1}%",
        conversation.conversation_id,
        conversation.model.as_deref().unwrap_or("-"),
        settings.window_minutes,
        conversation.window_start.to_rfc3339(),
        conversation.window_end.to_rfc3339(),
        conversation.turns,
        cache_rate,
        threshold,
    );
    MessageEnvelope::new(body).with_title(CACHE_ALERT_TITLE)
}

#[cfg(test)]
#[path = "cache_alert_sender_tests.rs"]
mod tests;
