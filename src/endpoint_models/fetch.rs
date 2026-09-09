use anyhow::{Context, Result, anyhow};
use reqwest::Client;

use crate::db::{EndpointProvider, RouteConfig};

use super::EndpointModelSnapshot;

pub async fn fetch_endpoint_model_ids(
    client: &Client,
    route: &RouteConfig,
) -> Result<EndpointModelSnapshot> {
    let response = client
        .get(models_url(&route.base_url, route.provider))
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

/// Build the upstream model-listing URL. GLM (issue #230 P2) lists its
/// models at `{base}/models` rather than the OpenAI-style `{base}/v1/models`
/// because the Zhipu Coding Plan base (`.../api/coding/paas/v4`) already
/// encodes the protocol root. Every other provider keeps the plain
/// `{base}/v1/models` join.
pub fn models_url(base_url: &str, provider: EndpointProvider) -> String {
    let base = base_url.trim_end_matches('/');
    match provider {
        EndpointProvider::Glm => format!("{base}/models"),
        _ => format!("{base}/v1/models"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::EndpointProvider;

    #[test]
    fn models_url_keeps_v1_for_non_glm_and_drops_for_glm() {
        // Every non-GLM provider joins the OpenAI-style `/v1/models`; GLM
        // (issue #230 P2) lists models at `/models` because the Coding
        // Plan base already encodes the protocol root. The non-GLM join
        // is intentionally plain (no `/v1` strip) so existing bases like
        // `https://example.com/api` keep their `/v1/models` convention.
        assert_eq!(
            models_url("https://example.com/api", EndpointProvider::Generic),
            "https://example.com/api/v1/models"
        );
        assert_eq!(
            models_url("https://api.openai.com/v1", EndpointProvider::Generic),
            "https://api.openai.com/v1/v1/models"
        );
        assert_eq!(
            models_url(
                "https://open.bigmodel.cn/api/coding/paas/v4",
                EndpointProvider::Glm
            ),
            "https://open.bigmodel.cn/api/coding/paas/v4/models"
        );
        assert_eq!(
            models_url(
                "https://api.z.ai/api/coding/paas/v4/",
                EndpointProvider::Glm
            ),
            "https://api.z.ai/api/coding/paas/v4/models"
        );
    }
}
