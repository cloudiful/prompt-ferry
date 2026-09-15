//! Issue #368 Phase E + #375 Phase F: outbound proxy for MCP upstreams.
//!
//! One-shot `rmcp` 3.3 conclusion (ACCEPTED): `StreamableHttpClientTransportConfig`
//! carries no proxy or client field and `from_config` builds rmcp's internal
//! default `reqwest` client (`pool_max_idle_per_host(0)`, `redirect none`,
//! auto system proxy enabled). Due to Cargo feature unification the compiled
//! `reqwest` includes this crate's `socks` + `system-proxy` features, so the
//! direct path is byte-for-byte identical only when no proxy env is set; when
//! proxy env is set but the upstream is bypassed, `from_config` re-enters
//! reqwest's own env matcher. Per-server proxy is impossible via config alone.
//! But `StreamableHttpClientTransport::with_client(client, config)` accepts any
//! `C: StreamableHttpClient`, and our `reqwest::Client` (with `socks`) implements
//! it, so per-server injection is feasible and used here.
//!
//! Actual MCP behavior (Phase F): `http` resolves per-row `proxy_url` first,
//! then process env (`HTTP_PROXY`/`HTTPS_PROXY` with `lowercase` accepted,
//! `ALL_PROXY` fallback), then direct. `NO_PROXY` bypasses both row and env
//! (exact/suffix/`*` fast-path decides direct; CIDR/IPv6 bypass per-request
//! via the pooled `NoProxy`). `builtin_minimax` inherits its
//! `source_endpoint_id` endpoint proxy (endpoint → env → direct) for the
//! MiniMax API request; its row proxy is ignored. `stdio` inherits the worker
//! env for the subprocess and never uses this pool (row proxy stored but
//! unused). Pooled per `(proxy, upstream host, NO_PROXY)` so each MCP server
//! gets its own client. Invalid row/endpoint proxy fails closed (never silent
//! direct, never fallback to env) with userinfo already scrubbed; empty row
//! falls through to the next level.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use reqwest::{Client, NoProxy, Proxy};

use crate::db;

type McpProxyPoolKey = (String, String, String);
type McpProxyPool = HashMap<McpProxyPoolKey, Client>;

static MCP_PROXY_POOL: OnceLock<Mutex<McpProxyPool>> = OnceLock::new();

fn pool() -> &'static Mutex<McpProxyPool> {
    MCP_PROXY_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_pool() -> std::sync::MutexGuard<'static, McpProxyPool> {
    pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn no_proxy_raw(get_env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    get_env("NO_PROXY").or_else(|| get_env("no_proxy"))
}

fn build_proxy_client(validated_proxy: &str, no_proxy: Option<&str>) -> Result<Client, String> {
    let redacted = db::redact_proxy_url_for_log(validated_proxy);
    let mut proxy = Proxy::all(validated_proxy)
        .map_err(|_| format!("invalid MCP proxy {redacted}: unsupported proxy URL"))?;
    if let Some(raw) = no_proxy.map(str::trim).filter(|value| !value.is_empty()) {
        proxy = proxy.no_proxy(NoProxy::from_string(raw));
    }
    Client::builder()
        .pool_max_idle_per_host(0)
        .redirect(reqwest::redirect::Policy::none())
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid MCP proxy {redacted}: failed to build proxy client"))
}

/// Issue #375 Phase F: pooled client with per-row proxy support.
pub(super) fn client_for_mcp_server_with_row(
    row_proxy: Option<&str>,
    upstream_url: &str,
) -> Result<Option<Client>, String> {
    client_for_mcp_server_with_row_and_env(row_proxy, upstream_url, &|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

// Unit-test seam for env-only resolution; production uses the row-aware path.
#[cfg(test)]
fn client_for_mcp_server_with_env(
    upstream_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Client>, String> {
    client_for_mcp_server_with_row_and_env(None, upstream_url, get_env)
}

fn client_for_mcp_server_with_row_and_env(
    row_proxy: Option<&str>,
    upstream_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Client>, String> {
    let Some(validated) = resolve_mcp_proxy_with_row(row_proxy, upstream_url, get_env)? else {
        return Ok(None);
    };
    let no_proxy = no_proxy_raw(get_env);
    let key = (
        validated.clone(),
        db::proxy_base_host(upstream_url),
        no_proxy.as_deref().unwrap_or_default().to_string(),
    );
    {
        let guard = lock_pool();
        if let Some(client) = guard.get(&key) {
            return Ok(Some(client.clone()));
        }
    }
    let client = build_proxy_client(&validated, no_proxy.as_deref())?;
    lock_pool().insert(key, client.clone());
    Ok(Some(client))
}

/// Issue #375 Phase F: resolve row proxy → env → direct. Non-empty row
/// validates (fail-closed, no env fallback); empty/`None` falls through to
/// env. `NO_PROXY` bypass applies to both levels (row bypass goes direct,
/// never falls back to env).
fn resolve_mcp_proxy_with_row(
    row_proxy: Option<&str>,
    upstream_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>, String> {
    if let Some(raw) = row_proxy.map(str::trim).filter(|v| !v.is_empty()) {
        if let Some(no_proxy) = no_proxy_raw(get_env) {
            let host = upstream_host(upstream_url);
            if should_bypass(&host, &no_proxy) {
                return Ok(None);
            }
        }
        return db::validate_outbound_proxy_url(raw)
            .map(Some)
            .map_err(|message| {
                format!(
                    "invalid MCP proxy {}: {message}",
                    db::redact_proxy_url_for_log(raw)
                )
            });
    }
    resolve_mcp_proxy_with_env(upstream_url, get_env)
}

// Unit-test seam for endpoint-proxy resolution; production goes through the row helper.
#[cfg(test)]
fn resolve_builtin_proxy_with_env(
    endpoint_proxy: Option<&str>,
    minimax_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>, String> {
    resolve_mcp_proxy_with_row(endpoint_proxy, minimax_url, get_env)
}

/// Issue #375 Phase F: one-off reqwest client for the builtin MiniMax API
/// request with endpoint-proxy → env → direct resolution. Direct builds the
/// same timeout-only client as before; proxied builds timeout + proxy with
/// the standard `NoProxy` attached. `Err` is redacted fail-closed.
pub(crate) fn client_for_builtin(
    endpoint_proxy: Option<&str>,
    minimax_host_url: &str,
    timeout_ms: i32,
) -> Result<reqwest::Client, String> {
    client_for_builtin_with_env(endpoint_proxy, minimax_host_url, timeout_ms, &|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn client_for_builtin_with_env(
    endpoint_proxy: Option<&str>,
    minimax_host_url: &str,
    timeout_ms: i32,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<reqwest::Client, String> {
    let resolved = resolve_mcp_proxy_with_row(endpoint_proxy, minimax_host_url, get_env)?;
    let timeout = std::time::Duration::from_millis(timeout_ms.max(100) as u64);
    let Some(validated) = resolved else {
        return reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| "invalid MCP proxy: failed to build direct client".to_string());
    };
    let redacted = db::redact_proxy_url_for_log(&validated);
    let mut proxy = reqwest::Proxy::all(&validated)
        .map_err(|_| format!("invalid MCP proxy {redacted}: unsupported proxy URL"))?;
    if let Some(raw) = no_proxy_raw(get_env)
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        proxy = proxy.no_proxy(reqwest::NoProxy::from_string(raw));
    }
    reqwest::Client::builder()
        .timeout(timeout)
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid MCP proxy {redacted}: failed to build proxy client"))
}

fn resolve_mcp_proxy_with_env(
    upstream_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>, String> {
    let Some(raw) = select_proxy_raw(upstream_url, get_env) else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    // Fast-path direct decision for the common `NO_PROXY` forms (exact host,
    // leading-dot suffix, `*`). The pooled proxy additionally carries
    // `NoProxy::from_string`, so CIDR, bare IPv6, and the rest of the standard
    // grammar still bypass per-request even when this check does not fire.
    if let Some(no_proxy) = no_proxy_raw(get_env) {
        let host = upstream_host(upstream_url);
        if should_bypass(&host, &no_proxy) {
            return Ok(None);
        }
    }
    db::validate_outbound_proxy_url(trimmed)
        .map(Some)
        .map_err(|message| {
            format!(
                "invalid MCP proxy {}: {message}",
                db::redact_proxy_url_for_log(trimmed)
            )
        })
}

fn select_proxy_raw(
    upstream_url: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let is_https = upstream_url
        .trim_ascii_start()
        .to_ascii_lowercase()
        .starts_with("https://");
    if is_https {
        for name in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
            if let Some(value) = get_env(name) {
                return Some(value);
            }
        }
    } else {
        for name in ["HTTP_PROXY", "http_proxy"] {
            if let Some(value) = get_env(name) {
                return Some(value);
            }
        }
    }
    get_env("ALL_PROXY").or_else(|| get_env("all_proxy"))
}

fn upstream_host(upstream_url: &str) -> String {
    db::proxy_base_host(upstream_url)
}

/// Fast-path `NO_PROXY` check for exact host, leading-dot/domain suffix, and
/// `*`. Full CIDR/IPv6/standard-grammar handling lives in the pooled proxy's
/// `NoProxy::from_string` (see `build_proxy_client`); this only decides the
/// `Ok(None)` direct fast-path so `from_config` can stay identical when no
/// proxy env is set.
fn should_bypass(host: &str, no_proxy: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    for entry in no_proxy.split(',') {
        let entry = entry.trim().to_ascii_lowercase();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return true;
        }
        let entry = entry.split(':').next().unwrap_or("").trim();
        if entry.is_empty() {
            continue;
        }
        if host == *entry {
            return true;
        }
        let suffix = entry.strip_prefix('.').unwrap_or(entry);
        if host.ends_with(suffix)
            && host.len() > suffix.len()
            && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn resolve_with(upstream: &str, pairs: &[(&str, &str)]) -> Result<Option<String>, String> {
        let map = env_map(pairs);
        resolve_mcp_proxy_with_env(upstream, &|name| map.get(name).cloned())
    }

    #[test]
    fn unset_env_means_direct() {
        assert_eq!(resolve_with("https://mcp.example.test", &[]).unwrap(), None);
    }

    #[test]
    fn https_prefers_https_proxy_then_http_proxy_then_all_proxy() {
        let upstream = "https://mcp.example.test";
        assert_eq!(
            resolve_with(upstream, &[("HTTPS_PROXY", "http://p1.test:8080")]).unwrap(),
            Some("http://p1.test:8080".to_string())
        );
        assert_eq!(
            resolve_with(
                upstream,
                &[
                    ("HTTP_PROXY", "http://p2.test:8080"),
                    ("ALL_PROXY", "http://p3.test:8080")
                ]
            )
            .unwrap(),
            Some("http://p2.test:8080".to_string())
        );
        assert_eq!(
            resolve_with(upstream, &[("ALL_PROXY", "http://p3.test:8080")]).unwrap(),
            Some("http://p3.test:8080".to_string())
        );
    }

    #[test]
    fn no_proxy_bypass_is_suffix_aware() {
        let upstream = "https://api.example.test/mcp";
        let proxy = [("HTTPS_PROXY", "http://proxy.test:8080")];
        assert!(resolve_with(upstream, &proxy).unwrap().is_some());
        for no_proxy in [
            "api.example.test",
            ".example.test",
            "*",
            "other.test, api.example.test",
        ] {
            let mut pairs = proxy.to_vec();
            pairs.push(("NO_PROXY", no_proxy));
            assert_eq!(
                resolve_with(upstream, &pairs).unwrap(),
                None,
                "NO_PROXY={no_proxy} must bypass"
            );
        }
        let mut pairs = proxy.to_vec();
        pairs.push(("NO_PROXY", "other.test"));
        assert!(resolve_with(upstream, &pairs).unwrap().is_some());
    }

    #[test]
    fn invalid_proxy_fails_closed_without_userinfo_leak() {
        let err = resolve_with(
            "https://mcp.example.test",
            &[("HTTPS_PROXY", "ftp://user:secret@proxy.test:21")],
        )
        .expect_err("ftp must be rejected");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    #[test]
    fn all_four_schemes_resolve() {
        for proxy in [
            "http://proxy.test:8080",
            "https://proxy.test:8443",
            "socks5://proxy.test:1080",
            "socks5h://user:secret@proxy.test:1080",
        ] {
            let resolved = resolve_with("https://mcp.example.test", &[("HTTPS_PROXY", proxy)])
                .expect("scheme must resolve");
            assert_eq!(resolved.as_deref(), Some(proxy));
            assert!(!db::redact_proxy_url_for_log(proxy).contains("secret"));
            // The MCP path must also build a pooled `reqwest` client for each
            // scheme (not just validate the string).
            build_proxy_client(proxy, None).expect("MCP proxy client must build");
        }
    }

    #[test]
    fn proxy_client_carries_reqwest_no_proxy_grammar() {
        // Entries the `should_bypass` fast-path does not understand (CIDR,
        // bare IPv6) must still bypass per-request via the attached
        // `NoProxy::from_string` instead of routing through the proxy.
        for no_proxy in ["10.0.0.0/8", "::1", "192.168.1.0/24"] {
            assert!(
                NoProxy::from_string(no_proxy).is_some(),
                "reqwest must accept NO_PROXY={no_proxy}"
            );
            build_proxy_client("http://proxy.test:8080", Some(no_proxy))
                .expect("proxy client with NO_PROXY must build");
        }
        // Fast-path still decides direct for the common forms.
        assert!(should_bypass("api.example.test", "api.example.test"));
        assert!(!should_bypass("api.example.test", "10.0.0.0/8"));
    }

    fn client_with(upstream: &str, pairs: &[(&str, &str)]) -> Result<Option<Client>, String> {
        let map = env_map(pairs);
        client_for_mcp_server_with_env(upstream, &|name| map.get(name).cloned())
    }

    fn resolve_row_with(
        row: Option<&str>,
        upstream: &str,
        pairs: &[(&str, &str)],
    ) -> Result<Option<String>, String> {
        let map = env_map(pairs);
        resolve_mcp_proxy_with_row(row, upstream, &|name| map.get(name).cloned())
    }

    fn builtin_with(
        endpoint: Option<&str>,
        minimax_url: &str,
        pairs: &[(&str, &str)],
    ) -> Result<Option<String>, String> {
        let map = env_map(pairs);
        resolve_builtin_proxy_with_env(endpoint, minimax_url, &|name| map.get(name).cloned())
    }

    // Issue #375 Phase F: row proxy overrides env.
    #[test]
    fn row_proxy_overrides_env() {
        let upstream = "https://mcp-row-override.test/mcp";
        let row = "http://row-proxy.test:8080";
        let env = "http://env-proxy.test:8080";
        let resolved =
            resolve_row_with(Some(row), upstream, &[("HTTPS_PROXY", env)]).expect("row must win");
        assert_eq!(resolved.as_deref(), Some(row));
    }

    // Issue #375 Phase F: empty row falls back to env.
    #[test]
    fn empty_row_falls_back_to_env() {
        let upstream = "https://mcp-empty-fallback.test/mcp";
        let env = "http://env-fallback.test:8080";
        for empty in [None, Some(""), Some("   ")] {
            let resolved = resolve_row_with(empty, upstream, &[("HTTPS_PROXY", env)])
                .expect("empty row must fallback");
            assert_eq!(
                resolved.as_deref(),
                Some(env),
                "row {empty:?} must fallback"
            );
        }
    }

    // Issue #375 Phase F: unset row and unset env means direct.
    #[test]
    fn unset_row_and_unset_env_means_direct() {
        let upstream = "https://mcp-unset-direct.test/mcp";
        assert_eq!(resolve_row_with(None, upstream, &[]).unwrap(), None);
        assert_eq!(resolve_row_with(Some("   "), upstream, &[]).unwrap(), None);
    }

    // Issue #375 Phase F: invalid row fails closed without userinfo leak
    // (never falls back to env).
    #[test]
    fn invalid_row_fails_closed_without_env_fallback() {
        let upstream = "https://mcp-invalid-row.test/mcp";
        let err = resolve_row_with(
            Some("ftp://user:secret@proxy.test:21"),
            upstream,
            &[("HTTPS_PROXY", "http://env-proxy.test:8080")],
        )
        .expect_err("invalid row must fail closed");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    // Issue #375 Phase F: builtin inherits its source endpoint proxy, with
    // empty falling back to env and unset meaning direct.
    #[test]
    fn builtin_inherits_source_endpoint_proxy() {
        let minimax = "https://api.minimaxi.com";
        let endpoint = "http://endpoint-proxy.test:8080";
        let env = "http://env-proxy.test:8080";
        // Endpoint wins over env.
        assert_eq!(
            builtin_with(Some(endpoint), minimax, &[("HTTPS_PROXY", env)]).unwrap(),
            Some(endpoint.to_string())
        );
        // Empty endpoint falls back to env.
        assert_eq!(
            builtin_with(Some("  "), minimax, &[("HTTPS_PROXY", env)]).unwrap(),
            Some(env.to_string())
        );
        // Unset endpoint and unset env means direct.
        assert_eq!(builtin_with(None, minimax, &[]).unwrap(), None);
        // Invalid endpoint fails closed without userinfo leak.
        let err = builtin_with(
            Some("socks4://user:secret@proxy.test:1080"),
            minimax,
            &[("HTTPS_PROXY", env)],
        )
        .expect_err("invalid endpoint proxy must fail closed");
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    #[test]
    fn injection_selection_uses_with_client_when_proxied_and_direct_when_unset() {
        // Direct arm: no proxy env means `Ok(None)` so the caller uses
        // `StreamableHttpClientTransport::from_config`.
        assert!(
            client_with("https://mcp-injection.test", &[])
                .unwrap()
                .is_none()
        );
        // Injected arm: proxy env means `Ok(Some)` so the caller uses
        // `StreamableHttpClientTransport::with_client`.
        let upstream = "https://mcp-injection-select.test";
        let proxy = "http://proxy-injection-select.test:8080";
        let selected = client_with(upstream, &[("HTTPS_PROXY", proxy)])
            .expect("proxied upstream must select a client");
        assert!(selected.is_some());
        // Same `(proxy, host, NO_PROXY)` reuses the pooled client.
        let again =
            client_with(upstream, &[("HTTPS_PROXY", proxy)]).expect("pooled client must be reused");
        assert!(again.is_some());
        // `NO_PROXY` fast-path bypass returns to the direct arm.
        let bypassed = client_with(
            upstream,
            &[
                ("HTTPS_PROXY", proxy),
                ("NO_PROXY", "mcp-injection-select.test"),
            ],
        )
        .expect("bypass must be direct");
        assert!(bypassed.is_none());
        // Invalid proxy fails closed without userinfo leak (never silent direct).
        let err = client_with(
            upstream,
            &[("HTTPS_PROXY", "ftp://user:secret@proxy.test:21")],
        )
        .expect_err("invalid proxy must fail closed");
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }
}
