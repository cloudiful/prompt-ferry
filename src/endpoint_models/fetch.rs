use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::{Client, Proxy};

use crate::db::{self, EndpointProvider, RouteConfig};
use crate::session_affinity::{apply_opencode_headers, opencode_headers_apply};

use super::EndpointModelSnapshot;
use super::cache::{CatalogAuth, CatalogFailure, CatalogFailureKind, CatalogKey};

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

/// One catalog read.
///
/// It carries the endpoint identity, the requested protocol, the credential
/// the read actually sends, and the request context the upstream call needs.
/// The cache identity of the read is derived from it, so a read and its cache
/// entry can never disagree about the endpoint, the protocol, or the
/// credential.
pub struct CatalogRequest<'a> {
    /// The endpoint the catalog belongs to.
    pub endpoint_id: uuid::Uuid,
    /// The provider, which selects the catalog shape.
    pub provider: EndpointProvider,
    /// The configured base of the endpoint.
    pub base_url: &'a str,
    /// The requested catalog protocol.
    pub native_api: crate::config::NativeApi,
    /// The credential this read actually sends.
    pub credential: &'a str,
    /// How the credential is sent.
    pub auth: CatalogAuth,
    /// The OpenCode routing session of the request.
    pub session: &'a str,
}

impl CatalogRequest<'_> {
    /// The cache identity of this read.
    pub fn key(&self) -> CatalogKey {
        CatalogKey::new(
            self.endpoint_id,
            self.provider,
            self.base_url,
            self.native_api,
            self.auth,
            self.credential,
        )
    }
}

/// The shared catalog read of one endpoint.
///
/// The provider and the protocol select the catalog URLs, the credential is
/// sent in its own mode, and an OpenCode endpoint carries its routing
/// session. Only a successfully parsed catalog is returned.
pub async fn fetch_catalog(
    client: &Client,
    request: &CatalogRequest<'_>,
) -> Result<EndpointModelSnapshot, CatalogFailure> {
    let mut model_ids = Vec::new();
    let mut seen = HashSet::new();
    for url in models_urls(request.base_url, request.provider, request.native_api) {
        for id in fetch_model_ids_from_url(client, request, &url).await? {
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
    request: &CatalogRequest<'_>,
    url: &str,
) -> Result<Vec<String>, CatalogFailure> {
    let builder = client.get(url);
    let builder = match request.auth {
        CatalogAuth::ApiKey => builder
            .header("x-api-key", request.credential)
            .header("anthropic-version", "2023-06-01"),
        CatalogAuth::Bearer => builder.bearer_auth(request.credential),
    };
    // Issue #1052 P1: the OpenCode gateway rejects a catalog read without a
    // routing session, so the read carries one stable non-secret session.
    let builder = if opencode_headers_apply(Some(request.provider), request.base_url)
        || opencode_headers_apply(Some(request.provider), url)
    {
        apply_opencode_headers(builder, &[], request.session)
    } else {
        builder
    };
    let response = builder
        .send()
        .await
        .map_err(|error| CatalogFailure::transport(format!("failed to fetch models: {error}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CatalogFailure::status(status.as_u16(), body.trim()));
    }

    let payload = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|_| CatalogFailure::parse("invalid /v1/models response"))?;
    let items = payload
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| CatalogFailure::parse("models response missing data array"))?;

    Ok(items
        .iter()
        .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect())
}

/// The runtime catalog read of one route.
///
/// The route's own credential is sent as a bearer token, and the failure
/// keeps the runtime's error wording.
pub async fn fetch_endpoint_model_ids(
    client: &Client,
    route: &RouteConfig,
) -> Result<EndpointModelSnapshot> {
    let session = format!("ses_{}", uuid::Uuid::new_v4());
    let request = CatalogRequest {
        endpoint_id: route.route_id,
        provider: route.provider,
        base_url: &route.base_url,
        native_api: route.native_api,
        credential: &route.api_key,
        auth: CatalogAuth::Bearer,
        session: &session,
    };
    fetch_catalog(client, &request)
        .await
        .map_err(|failure| catalog_failure_for_route(route, failure))
}

/// The runtime wording of one catalog failure.
fn catalog_failure_for_route(route: &RouteConfig, failure: CatalogFailure) -> anyhow::Error {
    match failure.kind {
        CatalogFailureKind::Transport => anyhow!(
            "failed to fetch models from endpoint {}: {}",
            route.route_id,
            failure.detail
        ),
        CatalogFailureKind::Status => anyhow!(
            "models request failed for endpoint {} with status {}: {}",
            route.route_id,
            failure.status.unwrap_or_default(),
            failure.detail
        ),
        CatalogFailureKind::Parse => {
            anyhow!("{} for endpoint {}", failure.detail, route.route_id)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{EndpointProvider, RouteSelectionReason};
    use std::sync::Arc;

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

    /// A route for the catalog fetch, with an explicit provider.
    fn route_for_provider(provider: EndpointProvider, base: &str) -> RouteConfig {
        RouteConfig {
            provider,
            base_url: base.to_string(),
            ..route_with_proxy(None, base)
        }
    }

    /// A loopback catalog upstream that records the OpenCode session header.
    async fn spawn_catalog_upstream() -> (String, Arc<Mutex<Option<String>>>) {
        let recorded = Arc::new(Mutex::new(None));
        let handler_recorded = recorded.clone();
        let app = axum::Router::new().route(
            "/v1/models",
            axum::routing::get(move |headers: axum::http::HeaderMap| {
                let handler_recorded = handler_recorded.clone();
                async move {
                    let session = headers
                        .get("x-opencode-session")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    *handler_recorded.lock().expect("recorded session") = session;
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({"data": [{"id": "gpt-4o"}]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}"), recorded)
    }

    fn catalog_request<'a>(
        provider: EndpointProvider,
        base: &'a str,
        credential: &'a str,
        session: &'a str,
    ) -> CatalogRequest<'a> {
        CatalogRequest {
            endpoint_id: uuid::Uuid::new_v4(),
            provider,
            base_url: base,
            native_api: crate::config::NativeApi::Chat,
            credential,
            auth: CatalogAuth::Bearer,
            session,
        }
    }

    #[tokio::test]
    async fn an_opencode_route_fetches_its_catalog_with_a_routing_session() {
        // Issue #1052 P1: the shared catalog fetcher reads an OpenCode
        // endpoint with a routing session, so the gateway accepts the read.
        let (base, recorded) = spawn_catalog_upstream().await;
        let route = route_for_provider(EndpointProvider::OpencodeGo, &base);

        let snapshot = fetch_endpoint_model_ids(&Client::new(), &route)
            .await
            .expect("catalog fetch");

        assert_eq!(snapshot.sorted_model_ids(), vec!["gpt-4o".to_string()]);
        let session = recorded
            .lock()
            .expect("recorded session")
            .clone()
            .expect("the catalog read carries a session");
        assert!(
            !session.trim().is_empty(),
            "the routing session must not be blank"
        );
    }

    #[tokio::test]
    async fn a_generic_route_fetches_its_catalog_without_an_opencode_session() {
        let (base, recorded) = spawn_catalog_upstream().await;
        let route = route_for_provider(EndpointProvider::Generic, &base);

        let snapshot = fetch_endpoint_model_ids(&Client::new(), &route)
            .await
            .expect("catalog fetch");

        assert_eq!(snapshot.sorted_model_ids(), vec!["gpt-4o".to_string()]);
        assert!(
            recorded.lock().expect("recorded session").is_none(),
            "a generic upstream must not receive an OpenCode session"
        );
    }

    #[tokio::test]
    async fn the_ordered_union_keeps_the_catalog_order() {
        // Issue #1052 P2 attempt 2: an automatic OpenRouter catalog is the
        // union of the chat and embeddings catalogs, in the order the
        // upstream listed them.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route(
                    "/v1/models",
                    axum::routing::get(|| async {
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({"data": [{"id": "zzz-chat"}]})),
                        )
                    }),
                )
                .route(
                    "/v1/embeddings/models",
                    axum::routing::get(|| async {
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({"data": [{"id": "aaa-embeddings"}]})),
                        )
                    }),
                );
            let _ = axum::serve(listener, app).await;
        });

        let base = format!("http://{address}");
        let request = CatalogRequest {
            endpoint_id: uuid::Uuid::new_v4(),
            provider: EndpointProvider::OpenRouter,
            base_url: &base,
            native_api: crate::config::NativeApi::Auto,
            credential: "sk-test",
            auth: CatalogAuth::Bearer,
            session: "ses_test",
        };
        let snapshot = fetch_catalog(&Client::new(), &request)
            .await
            .expect("catalog fetch");

        assert_eq!(
            snapshot.model_ids().collect::<Vec<_>>(),
            vec!["zzz-chat", "aaa-embeddings"],
            "the union keeps the catalog order"
        );
        assert_eq!(
            snapshot.sorted_model_ids(),
            vec!["aaa-embeddings".to_string(), "zzz-chat".to_string()]
        );
    }

    #[tokio::test]
    async fn a_failing_upstream_reports_its_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buffer = [0_u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buffer).await;
            let body = "upstream failure detail";
            let response = format!(
                "HTTP/1.1 502 Bad Gateway\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await;
        });

        let base = format!("http://{address}");
        let request = catalog_request(EndpointProvider::Generic, &base, "sk-test", "ses_test");
        let failure = fetch_catalog(&Client::new(), &request)
            .await
            .expect_err("the upstream failed");
        assert_eq!(failure.status, Some(502));
        assert_eq!(failure.kind, CatalogFailureKind::Status);
        assert!(failure.detail.contains("upstream failure detail"));
    }

    #[tokio::test]
    async fn the_runtime_failure_keeps_its_wording() {
        let route = route_with_proxy(None, "https://models.test");
        let failure = CatalogFailure::status(502, "upstream failure detail");
        let error = catalog_failure_for_route(&route, failure).to_string();
        assert!(error.contains("models request failed for endpoint"));
        assert!(error.contains("502"));
        assert!(error.contains("upstream failure detail"));
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
        // GLM (issue #230 P2) derives the Chat family root and lists
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
