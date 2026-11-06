//! Shared SSRF-safe address policy and DNS pinning for user-supplied
//! outbound URLs.
//!
//! The builtin MiniMax image tool and the personal MCP OAuth backend both talk
//! to a URL that is ultimately chosen by a user, so both resolve the host once,
//! reject any resolved address in a private, loopback, link-local, metadata or
//! otherwise reserved range, and then pin the request to exactly those
//! validated addresses. Centralising the policy keeps the two call sites from
//! drifting apart.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use ipnet::IpNet;
use reqwest::{Client, redirect};

/// The only native IPv6 allocation accepted for user-controlled destinations:
/// currently supported global unicast `2000::/3`.
pub(crate) const GLOBAL_UNICAST_V6: IpNet = IpNet::new_assert(
    IpAddr::V6(std::net::Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0)),
    3,
);

/// Every address range that is not global unicast, plus the transition
/// mechanisms that can embed an arbitrary IPv4 destination.
///
/// A user-controlled hostname must never reach shared address space, a
/// benchmark/documentation range, a reserved block or a tunnel that carries a
/// private IPv4 hop, so the list covers the IANA special-purpose ranges in
/// addition to the obvious private/loopback/link-local blocks.
pub(crate) const BLOCKED_NETS: &[IpNet] = &[
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 8),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 0)), 8),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(100, 64, 0, 0)), 10),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 0)), 8),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(169, 254, 0, 0)), 16),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(172, 16, 0, 0)), 12),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 0, 0)), 24),
    // TEST-NET documentation ranges.
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 0)), 24),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 0)), 24),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 0)), 24),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 0)), 16),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(192, 88, 99, 0)), 24),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(198, 18, 0, 0)), 15),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(224, 0, 0, 0)), 4),
    IpNet::new_assert(IpAddr::V4(std::net::Ipv4Addr::new(240, 0, 0, 0)), 4),
    IpNet::new_assert(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), 128),
    IpNet::new_assert(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), 128),
    // IPv4-compatible (deprecated) and discard-only prefixes.
    IpNet::new_assert(IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), 96),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x0100, 0, 0, 0, 0, 0, 0, 0)),
        64,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)),
        7,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)),
        10,
    ),
    // Deprecated IPv6 site-local.
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0)),
        10,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0)),
        8,
    ),
    // NAT64 prefixes embed an arbitrary IPv4 destination.
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0, 0)),
        96,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(
            0x0064, 0xff9b, 0x0001, 0, 0, 0, 0, 0,
        )),
        48,
    ),
    // Teredo, benchmarking, ORCHID, AS112 and other 2001 special-purpose
    // allocations; 2001:4860::/32 and other global unicast stay outside.
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0)),
        23,
    ),
    // IPv6 documentation, 6to4 (tunnels an arbitrary IPv4 destination) and
    // SRv6 SIDs.
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0)),
        32,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0)),
        16,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0)),
        20,
    ),
    IpNet::new_assert(
        IpAddr::V6(std::net::Ipv6Addr::new(0x5f00, 0, 0, 0, 0, 0, 0, 0)),
        16,
    ),
];

/// Hostnames that always point at a cloud metadata service regardless of what
/// they resolve to.
pub(crate) const CLOUD_METADATA_HOSTS: &[&str] = &[
    "metadata",
    "metadata.google.internal",
    "metadata.azure.internal",
];

/// Whether `ip` falls into a range that must never be reached from a
/// user-supplied URL.
///
/// IPv4-mapped IPv6 addresses are judged by their IPv4 counterpart, so
/// `::ffff:127.0.0.1` is blocked even when only the IPv6 form is supplied.
pub(crate) fn is_blocked_ip(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                // Native IPv6 is accepted only inside the currently supported
                // global-unicast allocation; everything else (site-local,
                // reserved, unallocated) is rejected even before the explicit
                // special-purpose deny list.
                if !GLOBAL_UNICAST_V6.contains(&IpAddr::V6(v6)) {
                    return true;
                }
                IpAddr::V6(v6)
            }
        },
        v4 => v4,
    };
    BLOCKED_NETS.iter().any(|net| net.contains(&ip))
}

/// Whether `host` names a cloud metadata service.
pub(crate) fn is_blocked_host(host: &str) -> bool {
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    CLOUD_METADATA_HOSTS.contains(&normalized.as_str())
}

/// Result of resolving a host against the blocked-address policy.
#[derive(Debug)]
pub(crate) enum ResolveError {
    /// The host resolved, but one of its addresses is blocked.
    Blocked { host: String, ip: IpAddr },
    /// The host did not resolve to any address.
    Empty { host: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blocked { host, ip } => {
                write!(formatter, "`{host}` resolves to a blocked address ({ip})")
            }
            Self::Empty { host } => write!(formatter, "`{host}` did not resolve"),
        }
    }
}

/// Resolve `host` and reject every blocked address, returning the addresses to
/// pin. The order of the returned addresses is preserved.
pub(crate) async fn resolve_and_validate(
    host: &str,
    port: u16,
) -> anyhow::Result<Result<Vec<SocketAddr>, ResolveError>> {
    resolve_and_validate_with(host, port, |host, port| async move {
        let resolved: Vec<SocketAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::net::lookup_host((host.clone(), port))
                .await
                .with_context(|| format!("failed to resolve host `{host}`"))?
                .collect(),
        };
        Ok(resolved)
    })
    .await
}

/// The validation boundary itself, with the resolver injected so a test can
/// exercise a mixed allowed/blocked answer set without a DNS server.
pub(crate) async fn resolve_and_validate_with<F, Fut>(
    host: &str,
    port: u16,
    resolve: F,
) -> anyhow::Result<Result<Vec<SocketAddr>, ResolveError>>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    resolve_with_policy(host, port, resolve, false).await
}

async fn resolve_with_policy<F, Fut>(
    host: &str,
    port: u16,
    resolve: F,
    allow_loopback: bool,
) -> anyhow::Result<Result<Vec<SocketAddr>, ResolveError>>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    if is_blocked_host(host) {
        return Ok(Err(ResolveError::Blocked {
            host: host.to_string(),
            ip: IpAddr::V4(std::net::Ipv4Addr::new(169, 254, 169, 254)),
        }));
    }
    let resolved = resolve(host.to_string(), port).await?;
    if resolved.is_empty() {
        return Ok(Err(ResolveError::Empty {
            host: host.to_string(),
        }));
    }
    if allow_loopback {
        // The test-only path accepts literal loopback only; the caller still
        // rejects every non-loopback answer after this policy check.
        if resolved.iter().any(|addr| !addr.ip().is_loopback()) {
            return Ok(Err(ResolveError::Blocked {
                host: host.to_string(),
                ip: resolved
                    .iter()
                    .map(SocketAddr::ip)
                    .find(|ip| !ip.is_loopback())
                    .unwrap_or(resolved[0].ip()),
            }));
        }
    } else if let Some(addr) = resolved.iter().find(|addr| is_blocked_ip(addr.ip())) {
        return Ok(Err(ResolveError::Blocked {
            host: host.to_string(),
            ip: addr.ip(),
        }));
    }
    Ok(Ok(resolved))
}

/// Resolve `host` without applying the blocked-address policy.
///
/// Only tests that stand up a mock authorization server on a loopback
/// address use this.
#[cfg(test)]
pub(crate) async fn resolve_host(host: &str, port: u16) -> anyhow::Result<Vec<SocketAddr>> {
    let resolved: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .with_context(|| format!("failed to resolve host `{host}`"))?
            .collect(),
    };
    if resolved.is_empty() {
        bail!("host `{host}` did not resolve");
    }
    Ok(resolved)
}

/// A client pinned to the exact addresses validated by
/// [`resolve_and_validate`].
///
/// Redirects are never followed and any proxy configured in the environment is
/// ignored, so an approved address cannot be redirected to an unvalidated one
/// and DNS is never re-resolved by a proxy.
pub(crate) fn client_with_pinned_dns(
    host: &str,
    resolved: &[SocketAddr],
    timeout: Duration,
) -> anyhow::Result<Client> {
    let timeout = timeout.max(Duration::from_millis(100));
    let builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(host, resolved);
    Ok(builder.build()?)
}

/// Validate that `url` is an HTTPS URL that may be fetched: HTTPS only, no
/// embedded credentials and no fragment.
pub(crate) fn validate_https_url(url: &reqwest::Url) -> anyhow::Result<()> {
    if url.scheme() != "https" {
        bail!("URL must use HTTPS");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("URL must not embed credentials");
    }
    if url.fragment().is_some() {
        bail!("URL must not include a fragment");
    }
    if url.host_str().filter(|host| !host.is_empty()).is_none() {
        bail!("URL must include a host");
    }
    Ok(())
}

/// Build a client pinned to the validated addresses of `url`.
///
/// The returned client never follows redirects, never uses an environment
/// proxy, and honours `timeout`.
pub(crate) async fn pinned_client_for_url(
    url: &reqwest::Url,
    timeout: Duration,
) -> anyhow::Result<Client> {
    pinned_client_for_url_with(url, timeout, |host, port| async move {
        let resolved: Vec<SocketAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::net::lookup_host((host.clone(), port))
                .await
                .with_context(|| format!("failed to resolve host `{host}`"))?
                .collect(),
        };
        Ok(resolved)
    })
    .await
}

/// The same validation -> resolver -> pinning path with an injected resolver.
/// Tests use it to model a first validated answer and a different later
/// answer; production uses [`pinned_client_for_url`] and the real resolver.
pub(crate) async fn pinned_client_for_url_with<F, Fut>(
    url: &reqwest::Url,
    timeout: Duration,
    resolve: F,
) -> anyhow::Result<Client>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    build_pinned_client(url, timeout, resolve, false).await
}

/// The single construction pipeline: URL policy, resolver, validate every
/// answer, then build the pinned client. `allow_loopback` is fixed false in
/// production and only the cfg(test) wrappers pass true; even then only
/// literal loopback answers are accepted.
async fn build_pinned_client<F, Fut>(
    url: &reqwest::Url,
    timeout: Duration,
    resolve: F,
    allow_loopback: bool,
) -> anyhow::Result<Client>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    if allow_loopback {
        if !matches!(url.scheme(), "http" | "https") {
            bail!("URL must use HTTP(S) in the loopback test path");
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            bail!("URL must not embed credentials or a fragment");
        }
    } else {
        validate_https_url(url)?;
    }
    let host = url
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| anyhow!("URL must include a host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let resolved = tokio::time::timeout(
        timeout.max(Duration::from_millis(100)),
        resolve_with_policy(host, port, resolve, allow_loopback),
    )
    .await
    .map_err(|_| anyhow!("resolving the URL host timed out"))?
    .map_err(|error| anyhow!("failed to resolve URL host: {error}"))?
    .map_err(|error| anyhow!("{error}"))?;
    if allow_loopback && resolved.iter().any(|addr| !addr.ip().is_loopback()) {
        bail!("the loopback test path accepts only loopback addresses");
    }
    client_with_pinned_dns(host, &resolved, timeout)
}

/// Test-only entry to the same construction pipeline with an injected
/// resolver. Only loopback answers are accepted.
#[cfg(test)]
pub(crate) async fn test_pinned_client_for_url_with<F, Fut>(
    url: &reqwest::Url,
    timeout: Duration,
    resolve: F,
) -> anyhow::Result<Client>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    build_pinned_client(url, timeout, resolve, true).await
}

/// Test-only constructor for a real loopback MCP transport. It accepts only
/// literal loopback answers and still validates, resolves and pins through the
/// shared helpers; production never uses this path.
#[cfg(test)]
pub(crate) async fn loopback_client_for_url(
    url: &reqwest::Url,
    timeout: Duration,
) -> anyhow::Result<Client> {
    test_pinned_client_for_url_with(url, timeout, |host, port| async move {
        resolve_host(&host, port).await
    })
    .await
}
