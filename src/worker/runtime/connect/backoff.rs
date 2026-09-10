use rand::RngExt;
use std::time::Duration;
use tokio::net::TcpStream;

use super::super::RELAY_RECONNECT_DELAY_SECONDS;

const RELAY_RECONNECT_MAX_DELAY_SECONDS: u64 = 60;
const RELAY_RECONNECT_MAX_JITTER_MILLIS: u64 = 1_000;
const RELAY_RECONNECT_JITTER_DIVISOR: u128 = 4;
/// Initial wait-for-ready budget: the worker tries a quick TCP probe to
/// the relay every 250ms for up to ~5s before falling back to the
/// exponential backoff. A TCP accept is enough — we still need the
/// WebSocket upgrade to succeed afterwards, but a successful accept
/// proves the listener is up and the TLS handshake has a chance.
pub(crate) const RELAY_READY_PROBE_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const RELAY_READY_PROBE_BUDGET: Duration = Duration::from_secs(5);

pub(crate) fn relay_reconnect_base_delay(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(31);
    let multiplier = 2_u64.saturating_pow(exponent);
    let seconds = RELAY_RECONNECT_DELAY_SECONDS
        .saturating_mul(multiplier)
        .min(RELAY_RECONNECT_MAX_DELAY_SECONDS);
    Duration::from_secs(seconds)
}

pub(crate) fn relay_reconnect_delay_with_jitter(consecutive_failures: u32) -> Duration {
    let base_delay = relay_reconnect_base_delay(consecutive_failures);
    let jitter_cap_millis = relay_reconnect_jitter_cap_millis(base_delay);
    let jitter_millis = if jitter_cap_millis == 0 {
        0
    } else {
        rand::rng().random_range(0..=jitter_cap_millis)
    };
    base_delay.saturating_add(Duration::from_millis(jitter_millis))
}

fn relay_reconnect_jitter_cap_millis(base_delay: Duration) -> u64 {
    ((base_delay.as_millis() / RELAY_RECONNECT_JITTER_DIVISOR)
        .min(u128::from(RELAY_RECONNECT_MAX_JITTER_MILLIS))) as u64
}

/// Try to TCP-connect to the relay repeatedly until the deadline. Used
/// to skip the exponential backoff when the relay just needs another
/// second or two to bind its listener (typical during `compose up`).
pub(crate) async fn wait_for_relay_ready(relay_url: &str) {
    wait_for_relay_ready_within(relay_url, RELAY_READY_PROBE_BUDGET).await;
}

pub(crate) async fn wait_for_relay_ready_within(relay_url: &str, budget: Duration) {
    let Some((host, port)) = relay_tcp_endpoint(relay_url) else {
        return;
    };
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let attempt = tokio::time::timeout(
            RELAY_READY_PROBE_INTERVAL,
            TcpStream::connect((host.as_str(), port)),
        );
        let Ok(Ok(stream)) = attempt.await else {
            if tokio::time::Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(RELAY_READY_PROBE_INTERVAL).await;
            continue;
        };
        drop(stream);
        return;
    }
}

fn relay_tcp_endpoint(relay_url: &str) -> Option<(String, u16)> {
    // We accept either the full WebSocket URL or a bare host:port; the
    // regex-free split keeps the parser dependency-free for a probe that
    // runs before the workspace's own URL crates are loaded.
    let trimmed = relay_url.trim();
    let scheme_end = trimmed.find("://")?;
    let after_scheme = &trimmed[scheme_end + 3..];
    let host_port_path = after_scheme
        .split('/')
        .next()
        .unwrap_or(after_scheme);
    if host_port_path.is_empty() {
        return None;
    }
    if let Some((host, port)) = host_port_path.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
    {
        let host = host.trim_matches(|c| c == '[' || c == ']');
        return Some((host.to_string(), port));
    }
    let default_port = if trimmed[..scheme_end].eq_ignore_ascii_case("wss") {
        443
    } else {
        80
    };
    Some((host_port_path.to_string(), default_port))
}

#[cfg(test)]
mod tests {
    use super::{
        RELAY_READY_PROBE_INTERVAL, relay_reconnect_base_delay,
        relay_reconnect_delay_with_jitter, relay_reconnect_jitter_cap_millis, relay_tcp_endpoint,
        wait_for_relay_ready_within,
    };
    use std::time::Duration;

    #[test]
    fn relay_reconnect_base_delay_grows_exponentially_and_caps() {
        assert_eq!(relay_reconnect_base_delay(1), Duration::from_secs(1));
        assert_eq!(relay_reconnect_base_delay(2), Duration::from_secs(2));
        assert_eq!(relay_reconnect_base_delay(3), Duration::from_secs(4));
        assert_eq!(relay_reconnect_base_delay(6), Duration::from_secs(32));
        assert_eq!(relay_reconnect_base_delay(7), Duration::from_secs(60));
        assert_eq!(relay_reconnect_base_delay(20), Duration::from_secs(60));
    }

    #[test]
    fn relay_reconnect_delay_caps_applied_jitter() {
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(1)),
            250
        );
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(4)),
            1_000
        );
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(60)),
            1_000
        );
    }

    #[test]
    fn relay_reconnect_delay_with_jitter_stays_in_expected_window() {
        let delay = relay_reconnect_delay_with_jitter(4);
        assert!(delay >= Duration::from_secs(8));
        assert!(delay <= Duration::from_secs(9));
    }

    #[test]
    fn relay_tcp_endpoint_parses_ws_urls_with_path_and_query() {
        let (host, port) =
            relay_tcp_endpoint("ws://relay:8788/ws/worker?token=abc").expect("parse");
        assert_eq!(host, "relay");
        assert_eq!(port, 8788);
        let (host, port) =
            relay_tcp_endpoint("wss://relay.example.com:443/ws/worker").expect("parse");
        assert_eq!(host, "relay.example.com");
        assert_eq!(port, 443);
    }

    #[test]
    fn relay_tcp_endpoint_defaults_port_from_scheme() {
        let (host, port) = relay_tcp_endpoint("ws://relay/ws/worker").expect("parse");
        assert_eq!(host, "relay");
        assert_eq!(port, 80);
        let (host, port) = relay_tcp_endpoint("wss://relay/ws/worker").expect("parse");
        assert_eq!(host, "relay");
        assert_eq!(port, 443);
    }

    #[tokio::test]
    async fn wait_for_relay_ready_returns_when_endpoint_accepts() {
        // The listener stays up for the whole probe: the first connect
        // attempt must succeed and the wait must return well within the
        // budget (not at the deadline).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://{}", addr);
        let budget = Duration::from_secs(2);
        let started = std::time::Instant::now();
        wait_for_relay_ready_within(&url, budget).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < budget,
            "wait_for_relay_ready should return on first accept; elapsed={elapsed:?}, budget={budget:?}"
        );
        drop(listener);
    }

    #[tokio::test]
    async fn wait_for_relay_ready_falls_back_after_budget_when_endpoint_refuses() {
        // Bind a listener, capture the port, and immediately release it so
        // the OS holds the socket in TIME_WAIT / closed state. With nothing
        // bound, every probe attempt fails — the wait must give up only
        // when its budget expires, not earlier (and not via panic).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://{}", addr);
        drop(listener);
        // Sleep one probe interval so the OS stops refusing the bind on
        // the same port; otherwise the next probe can be spuriously
        // accepted by a leftover accept queue.
        tokio::time::sleep(RELAY_READY_PROBE_INTERVAL * 2).await;
        let budget = Duration::from_millis(600);
        let started = std::time::Instant::now();
        wait_for_relay_ready_within(&url, budget).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed >= budget,
            "wait_for_relay_ready should honour its budget on probe failure; elapsed={elapsed:?}, budget={budget:?}"
        );
        // Sanity cap so a regression that hangs forever is caught quickly.
        assert!(
            elapsed < budget + RELAY_READY_PROBE_INTERVAL * 4 + Duration::from_millis(500),
            "wait_for_relay_ready lingered past the budget window; elapsed={elapsed:?}"
        );
    }
}
