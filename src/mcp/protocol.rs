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

#[cfg(test)]
mod tests {
    use super::{decode_resource_uri, encode_resource_uri};

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
}
