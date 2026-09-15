//! Issue #368 Phase D: per-`(proxy_url, base_host)` reqwest client pool.
//!
//! Direct routes (`None`/empty proxy) reuse the shared client unchanged so
//! the direct path is byte-for-byte identical to pre-proxy behavior.
//! Proxy routes get a pooled client keyed by trimmed proxy URL plus the
//! lowercased upstream host. All logging uses the redacted form.

use reqwest::{Client, Proxy};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::db;

static PROXY_CLIENT_POOL: OnceLock<Mutex<HashMap<(String, String), Client>>> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<(String, String), Client>> {
    PROXY_CLIENT_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_pool() -> std::sync::MutexGuard<'static, HashMap<(String, String), Client>> {
    pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build_proxy_client(validated_proxy: &str) -> Result<Client, String> {
    let redacted = db::redact_proxy_url_for_log(validated_proxy);
    let proxy = Proxy::all(validated_proxy)
        .map_err(|_| format!("invalid proxy {redacted}: unsupported proxy URL"))?;
    Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid proxy {redacted}: failed to build proxy client"))
}

/// Pooled client for an explicit proxy URL plus upstream base URL.
/// Returns `Err` with a redacted message when the proxy is invalid;
/// callers must not fall back to direct on error.
pub(super) fn client_for_proxy(proxy_url: &str, base_url: &str) -> Result<Client, String> {
    let trimmed = proxy_url.trim();
    let validated =
        db::validate_outbound_proxy_url(trimmed).map_err(|message| message.to_string())?;
    let key = (validated.clone(), db::proxy_base_host(base_url));
    {
        let guard = lock_pool();
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }
    let client = build_proxy_client(&validated)?;
    lock_pool().insert(key, client.clone());
    Ok(client)
}

/// Client for a resolved route: direct clones the shared client,
/// proxy routes use the pooled client. Invalid proxy is an error
/// (never silent direct fallback) with userinfo already scrubbed.
pub(super) fn client_for_route(direct: &Client, route: &db::RouteConfig) -> Result<Client, String> {
    let Some(proxy_url) = route
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(direct.clone());
    };
    client_for_proxy(proxy_url, &route.base_url)
}

/// Redacted proxy for `tracing` fields. `None`/empty maps to `""`
/// so callers can skip the field for direct routes.
pub(super) fn redacted_proxy_for_log(route: &db::RouteConfig) -> String {
    route
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(db::redact_proxy_url_for_log)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_with_proxy(proxy: Option<&str>, base: &str) -> db::RouteConfig {
        db::RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: base.to_string(),
            api_key: "k".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Chat,
            upstream_model: None,
            route_selection_reason: db::RouteSelectionReason::Default,
            provider: db::EndpointProvider::Generic,
            service_tier: db::MinimaxServiceTier::Standard,
            proxy_url: proxy.map(str::to_string),
            dev_system_normalize: false,
        }
    }

    #[test]
    fn empty_proxy_means_direct_without_pool_entry() {
        let direct = Client::new();
        for proxy in [None, Some(""), Some("   ")] {
            let route = route_with_proxy(proxy, "https://api.example.test");
            let selected = client_for_route(&direct, &route).expect("direct must succeed");
            let _ = selected;
            assert!(redacted_proxy_for_log(&route).is_empty());
        }
        assert!(db::proxy_pool_key("", "https://api.example.test").is_none());
        assert!(db::proxy_pool_key("  ", "https://api.example.test").is_none());
    }

    #[test]
    fn proxy_selection_keys_by_url_and_host() {
        let direct = Client::new();
        // Unique proxy host per test so parallel proxy tests share the
        // global pool without colliding.
        let proxy = "http://proxy-selection-keys.test:8080";
        let via_a = client_for_route(
            &direct,
            &route_with_proxy(Some(proxy), "https://a-selection-keys.example.test"),
        )
        .expect("proxy a");
        let via_a_again = client_for_route(
            &direct,
            &route_with_proxy(Some(proxy), "https://a-selection-keys.example.test"),
        )
        .expect("proxy a again");
        let via_b = client_for_route(
            &direct,
            &route_with_proxy(Some(proxy), "https://b-selection-keys.example.test"),
        )
        .expect("proxy b");
        let _ = (via_a, via_a_again, via_b);
        let key_a = db::proxy_pool_key(proxy, "https://a-selection-keys.example.test").unwrap();
        let key_b = db::proxy_pool_key(proxy, "https://b-selection-keys.example.test").unwrap();
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
            Some("ftp://user:secret@proxy.test:21"),
            "https://api.example.test",
        );
        let err = client_for_route(&direct, &route).expect_err("ftp must be rejected");
        assert!(err.contains("proxy_url scheme"));
        assert!(!err.contains("secret"));
        assert!(
            client_for_proxy(
                "gopher://proxy-invalid-scheme.test:70",
                "https://api.example.test"
            )
            .is_err()
        );
        assert!(db::validate_outbound_proxy_url("ftp://proxy.test:21").is_err());
        assert!(db::validate_outbound_proxy_url("http://").is_err());
    }

    #[test]
    fn all_four_schemes_build_pooled_clients() {
        for proxy in [
            "http://proxy-four-schemes.test:8080",
            "https://proxy-four-schemes.test:8443",
            "socks5://proxy-four-schemes.test:1080",
            "socks5h://user:secret@proxy-four-schemes.test:1080",
        ] {
            assert!(
                db::validate_outbound_proxy_url(proxy).is_ok(),
                "scheme must be accepted: {proxy}"
            );
            client_for_proxy(proxy, "https://api.example.test").expect("pooled client must build");
            // Redacted form must never carry the credential.
            assert!(!db::redact_proxy_url_for_log(proxy).contains("secret"));
        }
    }

    #[test]
    fn redaction_strips_userinfo_but_leaves_clean_urls() {
        let scrubbed = db::redact_proxy_url_for_log("http://user:secret@proxy.test:8080");
        assert!(!scrubbed.contains("secret"));
        assert!(!scrubbed.contains("user"));
        assert!(scrubbed.contains("proxy.test"));
        let clean = "https://proxy.test:8080";
        assert_eq!(db::redact_proxy_url_for_log(clean), clean);
        let socks = "socks5h://proxy.test:1080";
        assert_eq!(db::redact_proxy_url_for_log(socks), socks);
        assert_eq!(
            db::redact_proxy_url_for_log("not a url"),
            "[invalid-proxy-url]"
        );
    }
}
