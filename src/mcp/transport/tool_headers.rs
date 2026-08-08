//! SEP-2243 `Mcp-Param-*` header warmup and `-32020` recovery.
//!
//! rmcp caches tool input schemas (and only then emits `Mcp-Param-*` headers
//! for `tools/call`) from `tools/list` responses received on the *same*
//! transport. A freshly opened connection has an empty schema cache, so an
//! annotated call would be sent without its `Mcp-Param-*` headers and a
//! modern (2026-07-28) upstream would reject it with JSON-RPC `-32020`.
//!
//! The proxy therefore pre-lists tools on the calling transport before the
//! first annotated call, and on `-32020` re-lists and retries once. `-32020`
//! is raised before tool execution, so retrying cannot double-execute a tool;
//! ordinary tool errors are never retried here.

use rmcp::{
    ErrorData,
    model::{CallToolRequestParams, CallToolResponse, ProtocolVersion},
    service::ServiceError,
};

/// JSON-RPC error code for SEP-2243 header/body mismatches (HTTP 400).
const HEADER_MISMATCH_CODE: i32 = -32020;

/// True when `err` is a JSON-RPC `-32020` header mismatch from the peer.
pub(super) fn is_header_mismatch(err: &ServiceError) -> bool {
    matches!(
        err,
        ServiceError::McpError(ErrorData { code, .. }) if code.0 == HEADER_MISMATCH_CODE
    )
}

/// Calls a tool, warming the transport's schema cache first when the upstream
/// negotiated a protocol that mandates SEP-2243 standard headers.
///
/// `warmup` additionally requires the caller to only pass `true` for HTTP
/// transports: stdio has no HTTP headers, so pre-listing is pure overhead
/// there. Retrying on `-32020` is safe because the mismatch is detected before
/// the tool runs; other errors propagate unchanged.
pub(super) async fn call_tool_with_warmup(
    peer: &rmcp::Peer<rmcp::RoleClient>,
    params: CallToolRequestParams,
    protocol_version: &ProtocolVersion,
    warmup: bool,
) -> Result<CallToolResponse, ServiceError> {
    if warmup && *protocol_version >= ProtocolVersion::STANDARD_HEADERS {
        let _ = peer.list_all_tools().await?;
    }
    match peer.call_tool_once(params.clone()).await {
        Ok(response) => Ok(response),
        Err(err) if is_header_mismatch(&err) => {
            // The cache may be stale (the server refreshed its catalog since
            // the pre-list). Relist on the same connection and retry once.
            tracing::debug!(error = %err, "mcp tools/call rejected with header mismatch; relisting tools and retrying once");
            let _ = peer.list_all_tools().await?;
            peer.call_tool_once(params).await
        }
        Err(err) => Err(err),
    }
}
