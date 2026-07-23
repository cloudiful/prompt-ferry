use anyhow::{Context, Result, anyhow};
use reqwest::Client;

use crate::db::RouteConfig;

use super::EndpointModelSnapshot;

pub async fn fetch_endpoint_model_ids(
    client: &Client,
    route: &RouteConfig,
) -> Result<EndpointModelSnapshot> {
    let response = client
        .get(models_url(&route.base_url))
        .bearer_auth(&route.api_key)
        .send()
        .await
        .with_context(|| format!("failed to fetch models from endpoint {}", route.route_id))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "models request failed for endpoint {} with status {}: {}",
            route.route_id,
            status,
            truncate_message(body.trim())
        ));
    }

    let payload = serde_json::from_str::<serde_json::Value>(&body).with_context(|| {
        format!(
            "invalid /v1/models response for endpoint {}",
            route.route_id
        )
    })?;
    let items = payload
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            anyhow!(
                "models response missing data array for endpoint {}",
                route.route_id
            )
        })?;

    Ok(EndpointModelSnapshot::from_model_ids(
        items
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str)),
    ))
}

fn models_url(base_url: &str) -> String {
    format!("{}/v1/models", base_url.trim_end_matches('/'))
}

fn truncate_message(message: &str) -> String {
    const LIMIT: usize = 240;
    if message.chars().count() <= LIMIT {
        return message.to_string();
    }
    let mut truncated = String::new();
    for ch in message.chars().take(LIMIT - 3) {
        truncated.push(ch);
    }
    truncated.push_str("...");
    truncated
}
