//! SSRF-safe HTTP transport for the personal MCP OAuth backend.
//!
//! Every outbound OAuth request — metadata discovery, dynamic client
//! registration and the token exchange — goes through this client. It enforces
//! HTTPS, rejects embedded credentials and fragments, resolves the host once,
//! rejects private/link-local/metadata/loopback/reserved addresses (including
//! IPv4-mapped IPv6 forms), pins the connection to the validated addresses,
//! never follows redirects, never uses an environment proxy, and bounds both
//! time (30s) and body size (1 MiB).

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::{Client, Method, Url};
use rmcp::transport::auth::{
    OAuthHttpClient, OAuthHttpClientError, OAuthHttpClientFuture, OAuthHttpRequest,
};

use crate::mcp::transport::pinning;

/// Hard ceiling on one outbound OAuth response body.
pub(crate) const MAX_OAUTH_BODY_BYTES: usize = 1024 * 1024;
/// Hard ceiling on one outbound OAuth request.
pub(crate) const MAX_OAUTH_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
pub(crate) type TestResolver = std::sync::Arc<
    dyn Fn() -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Vec<std::net::SocketAddr>>> + Send>,
        > + Send
        + Sync,
>;

#[derive(Clone)]
pub(crate) struct PinnedOAuthHttpClient {
    #[cfg(test)]
    allow_private: bool,
    /// Test-only cap so a timeout regression does not sleep for 30 seconds.
    /// Compiled only for tests; production always uses the fixed ceiling.
    #[cfg(test)]
    timeout_override: Option<Duration>,
    /// Test-only resolver injection: a sequence of answers observed through
    /// the same validation and pinning path.
    #[cfg(test)]
    resolver: Option<TestResolver>,
}

impl PinnedOAuthHttpClient {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(test)]
            allow_private: false,
            #[cfg(test)]
            timeout_override: None,
            #[cfg(test)]
            resolver: None,
        }
    }

    /// Test-only client that accepts a loopback or private mock authorization
    /// server. Compiled only for tests; no production environment flag can
    /// enable it.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            allow_private: true,
            timeout_override: None,
            resolver: None,
        }
    }

    /// Test-only client whose resolver is injected. It still flows through
    /// address validation and DNS pinning; only the answer source changes.
    #[cfg(test)]
    pub(crate) fn for_test_with_resolver(resolver: TestResolver) -> Self {
        Self {
            allow_private: true,
            timeout_override: None,
            resolver: Some(resolver),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_resolver_and_timeout(
        resolver: TestResolver,
        timeout: Duration,
    ) -> Self {
        Self {
            allow_private: true,
            timeout_override: Some(timeout),
            resolver: Some(resolver),
        }
    }

    /// Test-only client with a short timeout cap for the bounded-time
    /// regression; `for_test` without an override keeps the production limit.
    #[cfg(test)]
    pub(crate) fn for_test_with_timeout(timeout: Duration) -> Self {
        Self {
            allow_private: true,
            timeout_override: Some(timeout),
            resolver: None,
        }
    }

    fn effective_timeout(&self, requested: Duration) -> Duration {
        #[cfg(test)]
        let ceiling = self.timeout_override.unwrap_or(MAX_OAUTH_TIMEOUT);
        #[cfg(not(test))]
        let ceiling = MAX_OAUTH_TIMEOUT;
        requested.min(ceiling)
    }

    async fn client_for(&self, url: &Url, timeout: Duration) -> Result<Client> {
        let timeout = self.effective_timeout(timeout);
        #[cfg(test)]
        if self.allow_private {
            if let Some(resolver) = self.resolver.clone() {
                return pinning::test_pinned_client_for_url_with(
                    url,
                    timeout,
                    move |_host, _port| async move { resolver().await },
                )
                .await;
            }
            return pinning::loopback_client_for_url(url, timeout).await;
        }
        pinning::pinned_client_for_url(url, timeout).await
    }

    /// POST a JSON body to `url` (used for dynamic client registration, whose
    /// rmcp request constructor is private).
    pub(crate) async fn post_json(&self, url: &str, body: &[u8]) -> Result<(u16, Vec<u8>)> {
        let url =
            Url::parse(url).map_err(|_| anyhow!("registration endpoint is not a valid URL"))?;
        let timeout = self.effective_timeout(MAX_OAUTH_TIMEOUT);
        tokio::time::timeout(timeout.max(Duration::from_millis(100)), async move {
            let client = self.client_for(&url, timeout).await?;
            let response = client
                .request(Method::POST, url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::ACCEPT, "application/json")
                .body(body.to_vec())
                .send()
                .await
                .map_err(|_| anyhow!("registration request failed"))?;
            let status = response.status().as_u16();
            let bytes = read_bounded(response).await?;
            Ok((status, bytes))
        })
        .await
        .map_err(|_| anyhow!("registration request timed out"))?
    }
}

/// Read a response body while enforcing [`MAX_OAUTH_BODY_BYTES`].
async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>> {
    let mut collected: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("OAuth response read failed"))?
    {
        if collected.len() + chunk.len() > MAX_OAUTH_BODY_BYTES {
            bail!(
                "OAuth response body exceeds the {} byte limit",
                MAX_OAUTH_BODY_BYTES
            );
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(collected)
}

fn boxed(message: String) -> OAuthHttpClientError {
    message.into()
}

impl OAuthHttpClient for PinnedOAuthHttpClient {
    fn execute(&self, request: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            let timeout = self.effective_timeout(request.timeout.unwrap_or(MAX_OAUTH_TIMEOUT));
            tokio::time::timeout(timeout.max(Duration::from_millis(100)), async move {
                let inner = request.request;
                let method = Method::from_bytes(inner.method().as_str().as_bytes())
                    .map_err(|_| boxed("OAuth request method is not valid".to_string()))?;
                let url = Url::parse(&inner.uri().to_string())
                    .map_err(|_| boxed("OAuth request URL is not valid".to_string()))?;
                let client = self
                    .client_for(&url, timeout)
                    .await
                    .map_err(|error| boxed(format!("OAuth request rejected: {error}")))?;

                let mut builder = client.request(method, url);
                for (name, value) in inner.headers() {
                    builder = builder.header(name, value);
                }
                let body = inner.into_body();
                if !body.is_empty() {
                    builder = builder.body(body);
                }
                let response = builder
                    .send()
                    .await
                    .map_err(|_| boxed("OAuth request failed".to_string()))?;
                let status = response.status().as_u16();
                let headers = response.headers().clone();
                let bytes = read_bounded(response)
                    .await
                    .map_err(|error| boxed(error.to_string()))?;

                let mut out = http::Response::builder().status(status);
                for (name, value) in headers.iter() {
                    out = out.header(name, value);
                }
                out.body(bytes)
                    .map_err(|_| boxed("OAuth response could not be built".to_string()))
            })
            .await
            .map_err(|_| boxed("OAuth request timed out".to_string()))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PinnedOAuthHttpClient, TestResolver};
    use axum::{Router, http::StatusCode, routing::post};
    use std::collections::VecDeque;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    fn resolver(
        answers: Arc<Mutex<VecDeque<Vec<SocketAddr>>>>,
        calls: Arc<AtomicUsize>,
    ) -> TestResolver {
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let next = answers
                .lock()
                .expect("answers")
                .pop_front()
                .unwrap_or_default();
            Box::pin(async move { Ok(next) })
        })
    }

    fn stalled_resolver() -> TestResolver {
        Arc::new(|| Box::pin(std::future::pending()))
    }

    async fn spawn_listener(address: &str, hits: Arc<AtomicUsize>) -> u16 {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("listener address").port();
        let app = Router::new().route(
            "/probe",
            post(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, "{}")
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        port
    }

    #[tokio::test]
    async fn the_validated_pinned_address_is_used_even_when_later_resolution_differs() {
        let pinned_hits = Arc::new(AtomicUsize::new(0));
        let alternate_hits = Arc::new(AtomicUsize::new(0));
        let port = spawn_listener("127.0.0.1:0", pinned_hits.clone()).await;
        spawn_listener(&format!("127.0.0.2:{port}"), alternate_hits.clone()).await;
        let a = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let b = SocketAddr::from((Ipv4Addr::new(127, 0, 0, 2), port));

        let answers = Arc::new(Mutex::new(VecDeque::from([vec![a], vec![b]])));
        let calls = Arc::new(AtomicUsize::new(0));
        let client =
            PinnedOAuthHttpClient::for_test_with_resolver(resolver(answers.clone(), calls.clone()));
        let url = format!("http://pinned.example.test:{port}/probe");
        let (status, body) = client.post_json(&url, b"{}").await.expect("pinned request");
        assert_eq!(status, 200);
        assert_eq!(body, b"{}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the validated answer is resolved exactly once"
        );
        assert_eq!(
            pinned_hits.load(Ordering::SeqCst),
            1,
            "the validated address must receive the request"
        );
        assert_eq!(
            alternate_hits.load(Ordering::SeqCst),
            0,
            "a different later answer must never be contacted"
        );
        assert!(
            answers.lock().expect("answers").len() == 1,
            "the alternate answer must remain unused"
        );

        // Non-vacuous control: the alternate listener is reachable and would
        // count the request when it is the validated answer.
        let control = PinnedOAuthHttpClient::for_test_with_resolver(resolver(
            Arc::new(Mutex::new(VecDeque::from([vec![b]]))),
            Arc::new(AtomicUsize::new(0)),
        ));
        control
            .post_json(&url, b"{}")
            .await
            .expect("control request");
        assert_eq!(alternate_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_test_resolver_cannot_smuggle_a_non_loopback_answer() {
        let port = spawn_listener("127.0.0.1:0", Arc::new(AtomicUsize::new(0))).await;
        let client = PinnedOAuthHttpClient::for_test_with_resolver(resolver(
            Arc::new(Mutex::new(VecDeque::from([vec![SocketAddr::from((
                Ipv4Addr::new(10, 0, 0, 1),
                port,
            ))]]))),
            Arc::new(AtomicUsize::new(0)),
        ));
        let error = client
            .post_json(&format!("http://pinned.example.test:{port}/probe"), b"{}")
            .await
            .expect_err("a private answer is refused");
        assert!(
            error.to_string().contains("blocked"),
            "a private answer is refused: {error}"
        );
    }

    async fn spawn_streaming_body(address: &str) -> u16 {
        use axum::body::Body;
        use axum::response::IntoResponse;
        use futures::StreamExt;

        let app = Router::new().route(
            "/stream",
            post(|| async {
                let first = futures::stream::once(async {
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(
                        b"{\"access_token\":\"partial\"",
                    ))
                });
                let rest = futures::stream::pending();
                (StatusCode::OK, Body::from_stream(first.chain(rest))).into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .expect("bind streaming listener");
        let port = listener.local_addr().expect("streaming address").port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        port
    }

    #[tokio::test]
    async fn a_delayed_body_chunk_after_headers_is_bounded() {
        let port = spawn_streaming_body("127.0.0.1:0").await;
        let client = PinnedOAuthHttpClient::for_test_with_resolver_and_timeout(
            resolver(
                Arc::new(Mutex::new(VecDeque::from([vec![SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    port,
                ))]]))),
                Arc::new(AtomicUsize::new(0)),
            ),
            std::time::Duration::from_millis(100),
        );
        let started = std::time::Instant::now();
        let error = client
            .post_json(&format!("http://stream.example.test:{port}/stream"), b"{}")
            .await
            .expect_err("a stalled body after headers must time out");
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the short test budget must bound the body read"
        );
    }

    #[tokio::test]
    async fn a_stalled_resolver_is_bounded_and_a_normal_request_still_succeeds() {
        let stalled = PinnedOAuthHttpClient::for_test_with_resolver_and_timeout(
            stalled_resolver(),
            std::time::Duration::from_millis(100),
        );
        let started = std::time::Instant::now();
        let error = stalled
            .post_json("http://pinned.example.test:80/probe", b"{}")
            .await
            .expect_err("a stalled resolver must time out");
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the short test budget must not wait for the 30s ceiling"
        );

        let hits = Arc::new(AtomicUsize::new(0));
        let port = spawn_listener("127.0.0.1:0", hits.clone()).await;
        let normal = PinnedOAuthHttpClient::for_test_with_resolver(resolver(
            Arc::new(Mutex::new(VecDeque::from([vec![SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                port,
            ))]]))),
            Arc::new(AtomicUsize::new(0)),
        ));
        normal
            .post_json(&format!("http://pinned.example.test:{port}/probe"), b"{}")
            .await
            .expect("normal control");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
