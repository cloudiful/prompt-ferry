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

#[derive(Clone)]
pub(crate) struct PinnedOAuthHttpClient;

impl PinnedOAuthHttpClient {
    pub(crate) fn new() -> Self {
        Self
    }

    fn effective_timeout(&self, requested: Duration) -> Duration {
        requested.min(MAX_OAUTH_TIMEOUT)
    }

    async fn client_for(&self, url: &Url, timeout: Duration) -> Result<Client> {
        let timeout = self.effective_timeout(timeout);
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
