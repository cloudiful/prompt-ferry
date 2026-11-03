//! The image-source handling of the builtin MiniMax image tool.
//!
//! The image source is user supplied, so it stays on HTTPS, its host is
//! resolved once and pinned to the validated addresses, and the fetched bytes
//! are limited and type checked before they are inlined into the request.

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use ipnet::IpNet;
use reqwest::{Client, redirect};
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

pub(super) const BLOCKED_NETS: &[IpNet] = &[
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0)), 16),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)), 12),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0)), 16),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0)), 4),
    IpNet::new_assert(IpAddr::V4(Ipv4Addr::new(240, 0, 0, 0)), 4),
    IpNet::new_assert(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 128),
    IpNet::new_assert(IpAddr::V6(Ipv6Addr::LOCALHOST), 128),
    IpNet::new_assert(IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)), 7),
    IpNet::new_assert(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)), 10),
    IpNet::new_assert(IpAddr::V6(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0)), 8),
];

/// Resolve an image source into an inline data URL.
///
/// The budget bounds the fetch, so the image preparation shares the tool
/// call deadline with the upstream attempts.
pub(super) async fn normalize_image_source(
    source: &str,
    budget: Duration,
) -> anyhow::Result<String> {
    let source = source.strip_prefix('@').unwrap_or(source);
    if source.starts_with("data:image/") {
        return Ok(source.to_string());
    }
    let url = reqwest::Url::parse(source)
        .map_err(|_error| anyhow!("image_source must be an HTTPS URL or data URL"))?;
    if url.scheme() != "https" {
        bail!("image_source URL must use HTTPS")
    }
    let host = url
        .host_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("image_source URL must include a host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let resolved: Vec<std::net::SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![std::net::SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .with_context(|| format!("failed to resolve image_source host `{host}`"))?
            .collect(),
    };
    if resolved.is_empty() {
        bail!("image_source host `{host}` did not resolve")
    }
    let mut any_blocked = None;
    for addr in &resolved {
        if is_blocked_ip(addr.ip()) {
            any_blocked = Some(addr.ip());
            break;
        }
    }
    if let Some(ip) = any_blocked {
        bail!("image_source host `{host}` resolves to a blocked address ({ip})")
    }
    let client = client_with_pinned_dns(host, &resolved, budget)?;
    // A request error repeats the full URL, including any userinfo or query
    // parameters, so it is replaced by a fixed description.
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_error| anyhow!("image_source fetch failed"))?
        .error_for_status()
        .map_err(|_error| anyhow!("image_source fetch failed"))?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|_error| anyhow!("image_source fetch failed"))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("image_source exceeds the 20 MiB limit")
    }
    let format = match content_type.as_str() {
        "image/png" | "image/webp" | "image/jpeg" => content_type,
        _ => bail!("image_source must be JPEG, PNG, or WebP"),
    };
    Ok(format!(
        "data:{format};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

pub(super) fn is_blocked_ip(ip: IpAddr) -> bool {
    BLOCKED_NETS.iter().any(|net| net.contains(&ip))
}

fn client_with_pinned_dns(
    host: &str,
    resolved: &[std::net::SocketAddr],
    budget: Duration,
) -> anyhow::Result<Client> {
    // Issue #375 Phase G: image-URL fetches stay direct by design (no
    // endpoint proxy context). The URL is user-supplied and SSRF-pinned via
    // `resolve_to_addrs` after blocklist validation; routing it through the
    // endpoint proxy would move DNS to the proxy and defeat the pinning.
    // Preserve the caller-configured timeout while removing redirect-following
    // and pinning the host to the addresses we just validated.
    let timeout = budget.max(Duration::from_millis(100));
    let builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(redirect::Policy::none())
        .resolve_to_addrs(host, resolved);
    Ok(builder.build()?)
}

/// The tool body of the image tool, with the resolved image URL.
pub(super) fn image_body(prompt: &str, image_url: &str) -> Value {
    serde_json::json!({ "prompt": prompt, "image_url": image_url })
}
