use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use ipnet::IpNet;
use reqwest::{Client, StatusCode, redirect};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{db, mcp::ServerCatalogSnapshot};

const SOURCE_HEADER: &str = "prompt-ferry-MiniMax-MCP";
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

const BLOCKED_NETS: &[IpNet] = &[
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

pub(crate) fn catalog() -> ServerCatalogSnapshot {
    ServerCatalogSnapshot {
        tools: vec![
            json!({
                "name": "web_search",
                "description": "Search the web with MiniMax Coding Plan.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            }),
            json!({
                "name": "understand_image",
                "description": "Analyze an image with MiniMax Coding Plan.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string" },
                        "image_source": { "type": "string" }
                    },
                    "required": ["prompt", "image_source"]
                }
            }),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    }
}

pub(crate) async fn call(
    storage: &crate::mcp::McpRuntimeStorage,
    server: &db::McpServer,
    request: &Value,
    conversation_id: Option<&str>,
) -> anyhow::Result<Value> {
    let endpoint_id = server
        .source_endpoint_id
        .ok_or_else(|| anyhow!("MiniMax MCP server has no source endpoint"))?;
    let endpoint = storage
        .repository()
        .get_endpoint_for_mcp(endpoint_id)
        .await?
        .ok_or_else(|| anyhow!("MiniMax MCP source endpoint not found"))?;
    if endpoint.provider != db::EndpointProvider::Minimax {
        bail!("MiniMax MCP source endpoint is not a MiniMax endpoint")
    }
    if !endpoint.enabled {
        bail!("MiniMax MCP source endpoint is disabled")
    }
    let available_keys = endpoint
        .api_keys
        .iter()
        .filter(|key| key.enabled && !key.api_key.trim().is_empty())
        .collect::<Vec<_>>();
    let selected_key = if endpoint.key_lb_enabled && available_keys.len() > 1 {
        let mut hasher = Sha256::new();
        hasher.update(conversation_id.unwrap_or("mcp-request").as_bytes());
        hasher.update(endpoint.endpoint_id.as_bytes());
        let index = (u64::from_be_bytes(
            hasher.finalize()[..8]
                .try_into()
                .expect("sha256 digest has eight bytes"),
        ) % available_keys.len() as u64) as usize;
        available_keys.get(index).copied()
    } else {
        available_keys.first().copied()
    };
    let (key, key_position) = selected_key
        .map(|key| (key.api_key.as_str(), key.position))
        .or_else(|| (!endpoint.api_key.trim().is_empty()).then_some((endpoint.api_key.as_str(), 0)))
        .filter(|(key, _)| !key.is_empty())
        .ok_or_else(|| anyhow!("MiniMax MCP source endpoint has no enabled API key"))?;
    crate::mcp::transport::record_builtin_token_slot(key_position.max(0) as usize);
    let name = request.pointer("/params/name").and_then(Value::as_str);
    let arguments = request
        .pointer("/params/arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let host = match endpoint.provider_region {
        Some(db::EndpointRegion::Cn) => "https://api.minimaxi.com",
        Some(db::EndpointRegion::Global) => "https://api.minimax.io",
        None => bail!("MiniMax MCP source endpoint has no region"),
    };
    let client = Client::builder()
        .timeout(Duration::from_millis(server.timeout_ms.max(100) as u64))
        .build()?;
    let (path, body) = match name {
        Some("web_search") => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("web_search requires a non-empty query"))?;
            ("/v1/coding_plan/search", json!({ "q": query }))
        }
        Some("understand_image") => {
            let prompt = arguments
                .get("prompt")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("understand_image requires a non-empty prompt"))?;
            let image_source = arguments
                .get("image_source")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("understand_image requires image_source"))?;
            let image_url = normalize_image_source(image_source, server.timeout_ms).await?;
            (
                "/v1/coding_plan/vlm",
                json!({ "prompt": prompt, "image_url": image_url }),
            )
        }
        Some(name) => bail!("unknown MiniMax MCP tool `{name}`"),
        None => bail!("MCP tools/call is missing params.name"),
    };
    let response = client
        .post(format!("{host}{path}"))
        .bearer_auth(key)
        .header("MM-API-Source", SOURCE_HEADER)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("MiniMax MCP request failed for {path}"))?;
    let status = response.status();
    let trace_id = response
        .headers()
        .get("Trace-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        return Err(http_error(status, &payload, trace_id.as_deref()));
    }
    let business_status = payload.pointer("/base_resp/status_code").and_then(as_i64);
    if business_status.unwrap_or(0) != 0 {
        let message = payload
            .pointer("/base_resp/status_msg")
            .and_then(Value::as_str)
            .unwrap_or("MiniMax API returned an error");
        bail!(
            "MiniMax API error {}: {}{}",
            business_status.unwrap_or(-1),
            message,
            trace_id
                .as_deref()
                .map(|id| format!(" (Trace-Id: {id})"))
                .unwrap_or_default()
        )
    }
    let text = if name == Some("understand_image") {
        payload
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("MiniMax VLM returned no content"))?
            .to_string()
    } else {
        serde_json::to_string_pretty(&payload)?
    };
    Ok(json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(Value::Null),
        "result": { "content": [{ "type": "text", "text": text }] }
    }))
}

fn as_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn http_error(status: StatusCode, payload: &Value, trace_id: Option<&str>) -> anyhow::Error {
    let message = payload
        .pointer("/base_resp/status_msg")
        .and_then(Value::as_str)
        .unwrap_or("MiniMax API request failed");
    anyhow!(
        "HTTP {}: MiniMax API error: {}{}",
        status.as_u16(),
        message,
        trace_id
            .map(|id| format!(" (Trace-Id: {id})"))
            .unwrap_or_default()
    )
}

async fn normalize_image_source(source: &str, timeout_ms: i32) -> anyhow::Result<String> {
    let source = source.strip_prefix('@').unwrap_or(source);
    if source.starts_with("data:image/") {
        return Ok(source.to_string());
    }
    let url =
        reqwest::Url::parse(source).context("image_source must be an HTTPS URL or data URL")?;
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
    let client = client_with_pinned_dns(host, &resolved, timeout_ms)?;
    let response = client.get(url).send().await?.error_for_status()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = response.bytes().await?;
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

fn is_blocked_ip(ip: IpAddr) -> bool {
    BLOCKED_NETS.iter().any(|net| net.contains(&ip))
}

fn client_with_pinned_dns(
    host: &str,
    resolved: &[std::net::SocketAddr],
    timeout_ms: i32,
) -> anyhow::Result<Client> {
    // Preserve the caller-configured timeout while removing redirect-following
    // and pinning the host to the addresses we just validated.
    let timeout = Duration::from_millis(timeout_ms.max(100) as u64);
    let builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(redirect::Policy::none())
        .resolve_to_addrs(host, resolved);
    Ok(builder.build()?)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{BLOCKED_NETS, catalog, is_blocked_ip};

    #[test]
    fn catalog_exposes_only_supported_minimax_tools() {
        let catalog = catalog();
        let names = catalog
            .tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names, ["web_search", "understand_image"]);
        assert!(catalog.resources.is_empty());
        assert!(catalog.prompts.is_empty());
    }

    #[test]
    fn blocks_ipv4_loopback_and_private_ranges() {
        let cases = [
            ("127.0.0.1", true),
            ("127.255.255.254", true),
            ("10.0.0.1", true),
            ("10.255.255.254", true),
            ("172.16.0.1", true),
            ("172.31.255.254", true),
            ("192.168.0.1", true),
            ("192.168.255.254", true),
            ("0.0.0.0", true),
            ("169.254.169.254", true),
            ("224.0.0.1", true),
            ("239.255.255.255", true),
            ("240.0.0.1", true),
            ("255.255.255.255", true),
            ("1.1.1.1", false),
            ("8.8.8.8", false),
            ("172.15.255.254", false),
            ("172.32.0.1", false),
            ("192.169.0.1", false),
        ];
        for (text, expected) in cases {
            let ip: IpAddr = text.parse().unwrap();
            assert_eq!(
                is_blocked_ip(ip),
                expected,
                "is_blocked_ip({ip}) should be {expected}",
            );
        }
    }

    #[test]
    fn blocks_ipv6_loopback_private_and_link_local_ranges() {
        let cases = [
            ("::1", true),
            ("::", true),
            ("fc00::1", true),
            ("fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", true),
            ("fe80::1", true),
            ("febf:ffff::", true),
            ("ff00::1", true),
            ("ff02::1", true),
            ("2001:db8::1", false),
            ("2606:4700:4700::1111", false),
        ];
        for (text, expected) in cases {
            let ip: IpAddr = text.parse().unwrap();
            assert_eq!(
                is_blocked_ip(ip),
                expected,
                "is_blocked_ip({ip}) should be {expected}",
            );
        }
    }

    #[test]
    fn blocked_nets_cover_every_documented_category() {
        // Belt-and-braces: every category present in the production ranges must
        // be represented in the const list so that future edits cannot silently
        // drop a range.
        let categories: &[(IpAddr, u8, &str)] = &[
            (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8, "unspecified"),
            (IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8, "private-10"),
            (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8, "loopback"),
            (IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0)), 16, "link-local"),
            (IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)), 12, "private-172"),
            (IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0)), 16, "private-192"),
            (IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0)), 4, "multicast"),
            (IpAddr::V4(Ipv4Addr::new(240, 0, 0, 0)), 4, "reserved"),
            (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 128, "v6-unspecified"),
            (IpAddr::V6(Ipv6Addr::LOCALHOST), 128, "v6-loopback"),
            (
                IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)),
                7,
                "v6-ula",
            ),
            (
                IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)),
                10,
                "v6-link-local",
            ),
            (
                IpAddr::V6(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0)),
                8,
                "v6-multicast",
            ),
        ];
        for (ip, prefix, label) in categories {
            let net = ipnet::IpNet::new_assert(*ip, *prefix);
            assert!(
                BLOCKED_NETS.iter().any(|existing| existing == &net),
                "missing blocked range {label} ({ip}/{prefix})",
            );
        }
    }
}
