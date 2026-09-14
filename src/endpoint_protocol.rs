use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use reqwest::{Client, Proxy};

use crate::db;

// Issue #375 Phase G: per-`(proxy_url, base_host)` client pool for the
// endpoint protocol check (`GET /v1/models`). Direct endpoints reuse the same
// 15s-timeout shape as before; proxy endpoints reuse a pooled 15s client
// keyed by trimmed proxy plus the lowercased upstream host. Errors are
// redacted; invalid proxy fails closed without falling back to direct.
static ENDPOINT_PROTOCOL_POOL: OnceLock<Mutex<HashMap<(String, String), Client>>> = OnceLock::new();

fn pool() -> &'static Mutex<HashMap<(String, String), Client>> {
    ENDPOINT_PROTOCOL_POOL.get_or_init(|| Mutex::new(HashMap::new()))
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
        .timeout(Duration::from_secs(15))
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid proxy {redacted}: failed to build proxy client"))
}

/// Direct protocol client (no endpoint context). Stays direct by design:
/// callers without an endpoint proxy (webhook-style user URLs, internal
/// control plane) must use this. Endpoint-aware callers must use
/// `endpoint_protocol_client_for_endpoint` so the endpoint proxy applies.
pub fn endpoint_protocol_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest client config is valid")
}

/// Issue #375 Phase G: pooled protocol client for an endpoint. Empty proxy
/// means direct with the same 15s timeout; non-empty validates (fail-closed).
pub fn endpoint_protocol_client_for_endpoint(
    proxy_url: Option<&str>,
    base_url: &str,
) -> Result<Client, String> {
    let Some(raw) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(endpoint_protocol_client());
    };
    let validated = db::validate_outbound_proxy_url(raw).map_err(|message| message.to_string())?;
    let key = (validated.clone(), db::proxy_base_host(base_url));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_proxy_means_direct_protocol_client() {
        for proxy in [None, Some(""), Some("   ")] {
            endpoint_protocol_client_for_endpoint(proxy, "https://protocol-direct.example.test")
                .expect("direct must succeed");
        }
        assert!(db::proxy_pool_key("", "https://protocol-direct.example.test").is_none());
    }

    #[test]
    fn proxy_selection_reuses_pooled_protocol_client() {
        let proxy = "http://proxy-protocol-reuse.test:8080";
        let host_a = "https://a-protocol-reuse.example.test";
        let host_b = "https://b-protocol-reuse.example.test";
        let via_a = endpoint_protocol_client_for_endpoint(Some(proxy), host_a).expect("proxy a");
        let via_a_again =
            endpoint_protocol_client_for_endpoint(Some(proxy), host_a).expect("proxy a again");
        let via_b = endpoint_protocol_client_for_endpoint(Some(proxy), host_b).expect("proxy b");
        let _ = (via_a, via_a_again, via_b);
        let key_a = db::proxy_pool_key(proxy, host_a).unwrap();
        let key_b = db::proxy_pool_key(proxy, host_b).unwrap();
        assert_eq!(key_a.0, key_b.0);
        assert_ne!(key_a.1, key_b.1);
        let guard = lock_pool();
        assert!(guard.contains_key(&key_a));
        assert!(guard.contains_key(&key_b));
    }

    #[test]
    fn invalid_protocol_proxy_is_rejected_without_userinfo_leak() {
        let err = endpoint_protocol_client_for_endpoint(
            Some("ftp://user:secret@proxy-protocol-invalid.test:21"),
            "https://protocol-invalid.example.test",
        )
        .expect_err("ftp must be rejected");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }
}
