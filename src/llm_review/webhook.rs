use reqwest::Client;

use crate::db;

use super::LlmReviewSettings;

pub fn approval_webhook_enabled(settings: &LlmReviewSettings) -> bool {
    settings.webhook.enabled && !settings.webhook.url.trim().is_empty()
}

pub fn spawn_approval_webhook(
    pool: sqlx::PgPool,
    client: Client,
    settings: LlmReviewSettings,
    event: &'static str,
    approval: db::ApprovalRequest,
) {
    if !approval_webhook_enabled(&settings) {
        return;
    }
    let webhook = settings.webhook;
    tokio::spawn(async move {
        let payload = serde_json::json!({
            "event": event,
            "approval": approval,
        });
        let mut last_error = None;
        for attempt in 0..3_u32 {
            let mut request = client
                .post(&webhook.url)
                .timeout(std::time::Duration::from_secs(10))
                .header(reqwest::header::CONTENT_TYPE, "application/json");
            if !webhook.bearer_token.trim().is_empty() {
                request = request.bearer_auth(&webhook.bearer_token);
            }
            for (name, value) in &webhook.extra_headers {
                request = request.header(name, value);
            }
            match request.json(&payload).send().await {
                Ok(response) if response.status().is_success() => {
                    let _ =
                        db::record_approval_webhook_result(&pool, approval.approval_id, None).await;
                    return;
                }
                Ok(response) => {
                    last_error = Some(format!("HTTP {}", response.status().as_u16()));
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200 * (1_u64 << attempt))).await;
        }
        if let Some(error) = last_error {
            tracing::warn!(approval_id = %approval.approval_id, error = %error, "approval webhook delivery failed");
            let _ =
                db::record_approval_webhook_result(&pool, approval.approval_id, Some(error)).await;
        }
    });
}
