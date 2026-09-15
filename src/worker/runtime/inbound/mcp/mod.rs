mod execution;
mod preparation;
mod redaction;
mod restore_failure;
mod streaming;

use super::super::context::RuntimeServices;
use super::super::request_assembly::BufferedMcpRequest;
use crate::{
    db,
    protocol::{BridgeMessage, McpResponseChunk, McpResponseEnd, McpResponseStart},
};

pub(super) async fn handle_mcp_request(request: BufferedMcpRequest, services: &RuntimeServices) {
    crate::mcp::with_tracked_credits(Box::pin(execution::execute_mcp_request(request, services)))
        .await
}

pub(super) async fn send_mcp_response(
    services: &RuntimeServices,
    request_id: &str,
    status: u16,
    content_type: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    let _ = services
        .out_tx
        .send(BridgeMessage::McpResponseStart(McpResponseStart {
            request_id: request_id.to_string(),
            status,
            content_type,
            headers,
        }))
        .await;
    if !body.is_empty() {
        let _ = services
            .out_tx
            .send(BridgeMessage::McpResponseChunk(McpResponseChunk {
                request_id: request_id.to_string(),
                data: body,
            }))
            .await;
    }
    let _ = services
        .out_tx
        .send(BridgeMessage::McpResponseEnd(McpResponseEnd {
            request_id: request_id.to_string(),
        }))
        .await;
}

/// Quota groups were removed (issue #414 Phase 2): MCP requests now always
/// follow the unconstrained credential path with legacy token balancing, so
/// there is no reservation to settle.
async fn settle_quota(
    _services: &RuntimeServices,
    _grant: Option<&db::McpCredential>,
    _request_id: uuid::Uuid,
    _status: u16,
) {
}

#[cfg(test)]
mod stack_tests;
