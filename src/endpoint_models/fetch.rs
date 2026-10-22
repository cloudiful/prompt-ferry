use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use reqwest::{Client, Proxy};

use crate::db::{self, EndpointProvider, RouteConfig};

use super::EndpointModelSnapshot;

// Issue #375 Phase G: per-`(proxy_url, base_host)` reqwest client pool for
// model-list fetches. Direct routes reuse the caller-passed client unchanged;
// proxy routes get a pooled client keyed by trimmed proxy URL plus the
// lowercased upstream host (same shape as #368 `worker::runtime::ai::proxy`).
// All errors are redacted (never echo userinfo); callers must not fall back
// to direct on error.
static MODELS_PROXY_POOL: OnceLock<Mutex<HashMap<(String, String), Client>>> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<(String, String), Client>> {
    MODELS_PROXY_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_pool() -> std::sync::MutexGuard<'static, HashMap<(String, String), Client>> {
    pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build_pooled_client(validated_proxy: &str) -> Result<Client, String> {
    let redacted = db::redact_proxy_url_for_log(validated_proxy);
    let proxy = Proxy::all(validated_proxy)
        .map_err(|_| format!("invalid proxy {redacted}: unsupported proxy URL"))?;
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid proxy {redacted}: failed to build proxy client"))
}

/// Issue #375 Phase G: pooled client for a model-list route. Direct
/// (`None`/empty proxy) clones `direct` unchanged so the direct path stays
/// identical; proxy routes use the pooled client. Invalid proxy is an error
/// (never silent direct fallback) with userinfo already scrubbed.
pub fn client_for_route(route: &RouteConfig, direct: &Client) -> Result<Client, String> {
    let Some(proxy_url) = route
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(direct.clone());
    };
    let validated =
        db::validate_outbound_proxy_url(proxy_url).map_err(|message| message.to_string())?;
    let key = (validated.clone(), db::proxy_base_host(&route.base_url));
    {
        let guard = lock_pool();
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }
    let client = build_pooled_client(&validated)?;
    lock_pool().insert(key, client.clone());
    Ok(client)
}

pub async fn fetch_endpoint_model_ids(
    client: &Client,
    route: &RouteConfig,
) -> Result<EndpointModelSnapshot> {
    let mut model_ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for url in models_urls(&route.base_url, route.provider, route.native_api) {
        for id in fetch_model_ids_from_url(client, route, &url).await? {
            if seen.insert(id.clone()) {
                model_ids.push(id);
            }
        }
    }
    Ok(EndpointModelSnapshot::from_model_ids(
        model_ids.iter().map(String::as_str),
    ))
}

async fn fetch_model_ids_from_url(
    client: &Client,
    route: &RouteConfig,
    url: &str,
) -> Result<Vec<String>> {
    let response = client
        .get(url)
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

    Ok(items
        .iter()
        .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect())
}

/// Build the upstream model-listing URL.
///
/// Preset providers derive their official base (issue #248), so a stored
/// base mangled by the legacy trailing-`/v1` strip self-heals. GLM lists its
/// models at `{base}/models` because the Chat family root already encodes the
/// protocol version; every other provider (including Generic) keeps the
/// plain `{base}/v1/models` join.
///
/// Issue #1032: an explicit `Embeddings` protocol lists the provider's
/// embeddings catalog where it differs from the chat catalog (OpenRouter:
/// `{base}/v1/embeddings/models`). Every chat protocol keeps the chat listing.
pub fn models_url(
    base_url: &str,
    provider: EndpointProvider,
    native_api: crate::config::NativeApi,
) -> String {
    // The base derivation keeps the pre-#1032 behavior (Chat family root) for
    // every protocol; only the catalog path varies for embeddings.
    let base = crate::upstream_presets::route_base_or_stored(
        provider.into(),
        base_url,
        crate::config::NativeApi::Chat.into(),
    );
    let base = base.trim_end_matches('/');
    match provider {
        EndpointProvider::Glm => format!("{base}/models"),
        EndpointProvider::OpenRouter if native_api == crate::config::NativeApi::Embeddings => {
            format!("{base}/v1/embeddings/models")
        }
        _ => format!("{base}/v1/models"),
    }
}

/// Issue #1032: every catalog URL that applies to one endpoint/target.
///
/// An `Auto` OpenRouter endpoint serves chat and embeddings, so its listing is
/// the deduplicated union of both catalogs. Every other combination has a
/// single catalog: the embeddings list for an explicit embeddings protocol,
/// the chat list otherwise.
pub fn models_urls(
    base_url: &str,
    provider: EndpointProvider,
    native_api: crate::config::NativeApi,
) -> Vec<String> {
    if provider == EndpointProvider::OpenRouter && native_api == crate::config::NativeApi::Auto {
        return vec![
            models_url(base_url, provider, crate::config::NativeApi::Chat),
            models_url(base_url, provider, crate::config::NativeApi::Embeddings),
        ];
    }
    vec![models_url(base_url, provider, native_api)]
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
    use crate::db::{EndpointProvider, RouteSelectionReason};

    fn route_with_proxy(proxy: Option<&str>, base: &str) -> RouteConfig {
        RouteConfig {
            embedding_dimensions_override: None,
            route_id: uuid::Uuid::new_v4(),
            user_id: 7,
            model_route_rule_id: None,
            base_url: base.to_string(),
            api_key: "secret".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Chat,
            upstream_model: None,
            route_selection_reason: RouteSelectionReason::Default,
            provider: EndpointProvider::Generic,
            service_tier: None,
            proxy_url: proxy.map(str::to_string),
            dev_system_normalize: false,
            thinking_effort_override: None,
            compact_mode: crate::db::CompactMode::Passthrough,
            thinking_downgrade_enabled: false,
            has_oauth_token: false,
        }
    }

    #[test]
    fn empty_proxy_means_direct_without_pool_entry() {
        let direct = Client::new();
        for proxy in [None, Some(""), Some("   ")] {
            let route = route_with_proxy(proxy, "https://models-direct.test");
            client_for_route(&route, &direct).expect("direct must succeed");
        }
        assert!(db::proxy_pool_key("", "https://models-direct.test").is_none());
    }

    #[test]
    fn proxy_selection_reuses_pooled_client() {
        let direct = Client::new();
        let proxy = "http://proxy-models-reuse.test:8080";
        let via_a = client_for_route(
            &route_with_proxy(Some(proxy), "https://a-models-reuse.example.test"),
            &direct,
        )
        .expect("proxy a");
        let via_a_again = client_for_route(
            &route_with_proxy(Some(proxy), "https://a-models-reuse.example.test"),
            &direct,
        )
        .expect("proxy a again");
        let via_b = client_for_route(
            &route_with_proxy(Some(proxy), "https://b-models-reuse.example.test"),
            &direct,
        )
        .expect("proxy b");
        let _ = (via_a, via_a_again, via_b);
        let key_a = db::proxy_pool_key(proxy, "https://a-models-reuse.example.test").unwrap();
        let key_b = db::proxy_pool_key(proxy, "https://b-models-reuse.example.test").unwrap();
        assert_eq!(key_a.0, key_b.0);
        assert_ne!(key_a.1, key_b.1);
        let guard = lock_pool();
        assert!(guard.contains_key(&key_a));
        assert!(guard.contains_key(&key_b));
    }

    #[test]
    fn invalid_scheme_is_rejected_without_userinfo_leak() {
        let direct = Client::new();
        let route = route_with_proxy(
            Some("ftp://user:secret@proxy-models-invalid.test:21"),
            "https://models-invalid.example.test",
        );
        let err = client_for_route(&route, &direct).expect_err("ftp must be rejected");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    #[test]
    fn caller_passing_selects_per_route_proxy() {
        // The Phase G caller pattern: each route resolves its own pooled
        // client from `proxy_url`; direct routes reuse the passed client.
        let direct = Client::new();
        let proxy = "http://proxy-models-caller.test:8080";
        let proxied = route_with_proxy(Some(proxy), "https://caller-proxied.example.test");
        let plain = route_with_proxy(None, "https://caller-direct.example.test");
        client_for_route(&proxied, &direct).expect("proxied caller must succeed");
        client_for_route(&plain, &direct).expect("direct caller must succeed");
        let key = db::proxy_pool_key(proxy, "https://caller-proxied.example.test").unwrap();
        assert!(lock_pool().contains_key(&key));
        assert!(db::proxy_pool_key("", "https://caller-direct.example.test").is_none());
    }

    #[test]
    fn models_url_keeps_v1_for_generic_and_derives_for_presets() {
        // Generic joins the OpenAI-style `/v1/models` from the stored base;
        // a stored `/api` base intentionally keeps its `/v1/models`
        // convention (the base is not normalized for models listing).
        assert_eq!(
            models_url(
                "https://example.com/api",
                EndpointProvider::Generic,
                crate::config::NativeApi::Chat
            ),
            "https://example.com/api/v1/models"
        );
        assert_eq!(
            models_url(
                "https://api.openai.com/v1",
                EndpointProvider::Generic,
                crate::config::NativeApi::Chat
            ),
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
                models_url(
                    stored,
                    EndpointProvider::Glm,
                    crate::config::NativeApi::Chat
                ),
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "stored GLM base {stored}"
            );
        }
        // Other presets self-heal a mangled stored `/v1` suffix.
        assert_eq!(
            models_url(
                "https://openrouter.ai/api/v1",
                EndpointProvider::OpenRouter,
                crate::config::NativeApi::Chat
            ),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            models_url(
                "https://api.commandcode.ai/provider/v1",
                EndpointProvider::CommandCode,
                crate::config::NativeApi::Chat
            ),
            "https://api.commandcode.ai/provider/v1/models"
        );
        assert_eq!(
            models_url(
                "https://api.minimaxi.com",
                EndpointProvider::Minimax,
                crate::config::NativeApi::Chat
            ),
            "https://api.minimaxi.com/v1/models"
        );
    }

    #[test]
    fn embeddings_catalogs_are_protocol_distinct() {
        // Issue #1032: OpenRouter exposes a separate embeddings catalog; an
        // Auto OpenRouter endpoint lists the deduplicated union of both.
        assert_eq!(
            models_url(
                "https://openrouter.ai/api/v1",
                EndpointProvider::OpenRouter,
                crate::config::NativeApi::Embeddings,
            ),
            "https://openrouter.ai/api/v1/embeddings/models"
        );
        let urls = models_urls(
            "https://openrouter.ai/api/v1",
            EndpointProvider::OpenRouter,
            crate::config::NativeApi::Auto,
        );
        assert_eq!(
            urls,
            vec![
                "https://openrouter.ai/api/v1/models".to_string(),
                "https://openrouter.ai/api/v1/embeddings/models".to_string(),
            ]
        );
        // A generic endpoint keeps the OpenAI-compatible path for both.
        assert_eq!(
            models_urls(
                "https://example.com/api",
                EndpointProvider::Generic,
                crate::config::NativeApi::Embeddings,
            ),
            vec!["https://example.com/api/v1/models".to_string()]
        );
    }
}
