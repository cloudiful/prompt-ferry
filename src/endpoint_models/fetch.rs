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

/// Build the upstream model-listing URL.
///
/// Preset providers derive their official base (issue #248), so a stored
/// base mangled by the legacy trailing-`/v1` strip self-heals. GLM lists its
/// models at `{base}/models` because the Chat family root already encodes the
/// protocol version; every other provider (including Generic) keeps the
/// plain `{base}/v1/models` join.
pub fn models_url(base_url: &str, provider: EndpointProvider) -> String {
    let base = crate::upstream_presets::route_base_or_stored(
        provider,
        base_url,
        crate::config::NativeApi::Chat,
    );
    let base = base.trim_end_matches('/');
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
    fn models_url_keeps_v1_for_generic_and_derives_for_presets() {
        // Generic joins the OpenAI-style `/v1/models` from the stored base;
        // a stored `/api` base intentionally keeps its `/v1/models`
        // convention (the base is not normalized for models listing).
        assert_eq!(
            models_url("https://example.com/api", EndpointProvider::Generic),
            "https://example.com/api/v1/models"
        );
        assert_eq!(
            models_url("https://api.openai.com/v1", EndpointProvider::Generic),
            "https://api.openai.com/v1/v1/models"
        );
        // GLM (issue #230 P2 / #248) derives the Chat family root and lists
        // at `{base}/models`; the api.z.ai mirror is dropped for the
        // domestic open.bigmodel.cn root.
        for stored in [
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "https://api.z.ai/api/coding/paas/v4/",
            "https://open.bigmodel.cn/api",
        ] {
            assert_eq!(
                models_url(stored, EndpointProvider::Glm),
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "stored GLM base {stored}"
            );
        }
        // Other presets self-heal a mangled stored `/v1` suffix.
        assert_eq!(
            models_url("https://openrouter.ai/api/v1", EndpointProvider::OpenRouter),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            models_url(
                "https://api.commandcode.ai/provider/v1",
                EndpointProvider::CommandCode
            ),
            "https://api.commandcode.ai/provider/v1/models"
        );
        assert_eq!(
            models_url("https://api.minimaxi.com", EndpointProvider::Minimax),
            "https://api.minimaxi.com/v1/models"
        );
    }
}
