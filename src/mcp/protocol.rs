use anyhow::Context;
use rmcp::model::{
    ClientCapabilities, ClientInfo, Implementation, InitializeResult, ProtocolVersion,
    ServerCapabilities,
};
use serde_json::{Value, json};

use crate::naming::{MCP_IMPLEMENTATION_NAME, MCP_SERVER_NAME};

pub(super) const DEFAULT_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_11_25;
pub(super) const DEFAULT_PROTOCOL_VERSION_STR: &str = "2025-11-25";

pub(super) fn json_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub(super) fn json_error_value(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub(super) fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new(MCP_IMPLEMENTATION_NAME, env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(DEFAULT_PROTOCOL_VERSION)
}

pub(super) fn aggregate_initialize_result() -> anyhow::Result<Value> {
    let capabilities = ServerCapabilities::builder()
        .enable_tools()
        .enable_resources()
        .enable_prompts()
        .enable_completions()
        .build();
    let result = InitializeResult::new(capabilities)
        .with_protocol_version(DEFAULT_PROTOCOL_VERSION)
        .with_server_info(Implementation::new(
            MCP_SERVER_NAME,
            env!("CARGO_PKG_VERSION"),
        ));
    Ok(serde_json::to_value(result)?)
}

pub(super) fn encode_resource_uri(server_name: &str, upstream_uri: &str) -> String {
    format!(
        "mcp://{}/{}",
        server_name,
        urlencoding::encode(upstream_uri)
    )
}

pub(super) fn decode_resource_uri(uri: &str) -> anyhow::Result<Option<(String, String)>> {
    let Some(rest) = uri.strip_prefix("mcp://") else {
        return Ok(None);
    };
    let Some((server_name, upstream_uri)) = rest.split_once('/') else {
        return Ok(None);
    };
    Ok(Some((
        server_name.to_string(),
        urlencoding::decode(upstream_uri)
            .context("invalid encoded resource uri")?
            .into_owned(),
    )))
}

/// Namespaces an upstream resource template so the aggregate `uriTemplate`
/// stays reversible while preserving RFC 6570 `{...}` expressions.
///
/// The server segment is fully percent-encoded (so it can never contain a raw
/// `/`), and the template segment keeps `{`/`}` intact while everything else
/// is percent-encoded. Splitting on the first `/` after `mcp://` therefore
/// yields exactly `(server, encoded-template)`.
pub(super) fn encode_resource_template_uri(server_name: &str, upstream_template: &str) -> String {
    format!(
        "mcp://{}/{}",
        urlencoding::encode(server_name),
        encode_template_segment(upstream_template)
    )
}

pub(super) fn decode_resource_template_uri(uri: &str) -> anyhow::Result<Option<(String, String)>> {
    let Some(rest) = uri.strip_prefix("mcp://") else {
        return Ok(None);
    };
    let Some((server_name, upstream_template)) = rest.split_once('/') else {
        return Ok(None);
    };
    Ok(Some((
        urlencoding::decode(server_name)
            .context("invalid encoded resource template server name")?
            .into_owned(),
        decode_template_segment(upstream_template)
            .context("invalid encoded resource template uri")?,
    )))
}

fn encode_template_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'{' | b'}' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn decode_template_segment(value: &str) -> anyhow::Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes
                    .get(index + 1..index + 3)
                    .ok_or_else(|| anyhow::anyhow!("truncated percent escape"))?;
                let hex = std::str::from_utf8(hex)
                    .map_err(|_| anyhow::anyhow!("invalid percent escape"))?;
                out.push(
                    u8::from_str_radix(hex, 16)
                        .map_err(|_| anyhow::anyhow!("invalid percent escape: %{hex}"))?,
                );
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| anyhow::anyhow!("template segment is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_resource_template_uri, decode_resource_uri, encode_resource_template_uri,
        encode_resource_uri,
    };

    #[test]
    fn resource_uri_round_trips_special_characters() {
        let upstream = "file:///tmp/a b/config.json?x=1&y=/nested";
        let routed = encode_resource_uri("context7", upstream);

        assert_eq!(
            decode_resource_uri(&routed).unwrap(),
            Some(("context7".to_string(), upstream.to_string()))
        );
    }

    #[test]
    fn resource_uri_rejects_non_mcp_scheme() {
        assert_eq!(decode_resource_uri("file:///tmp/a").unwrap(), None);
    }

    #[test]
    fn template_uri_preserves_rfc6570_expressions() {
        let template = "git://{owner}/{repo}/issues?state={state}";
        let routed = encode_resource_template_uri("github", template);

        assert!(
            routed.contains("{owner}") && routed.contains("{state}"),
            "RFC 6570 expressions must survive namespacing: {routed}"
        );
        assert!(
            !routed.contains("git://"),
            "template must be percent-encoded: {routed}"
        );
        assert_eq!(
            decode_resource_template_uri(&routed).unwrap(),
            Some(("github".to_string(), template.to_string()))
        );
    }

    #[test]
    fn template_uri_rejects_truncated_escape() {
        assert!(decode_resource_template_uri("mcp://github/git%3A%2F%2F%7").is_err());
    }
}
