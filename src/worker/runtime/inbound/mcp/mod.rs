mod execution;
mod preparation;
mod redaction;
mod restore_failure;
mod streaming;

use super::super::context::RuntimeServices;
use super::super::request_assembly::BufferedMcpRequest;
use crate::{
    db, mcp,
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

/// Settle a quota reservation after the upstream outcome is known: commit on
/// 2xx, release otherwise. Auth/throttle failures additionally put the
/// credential into cooldown so later requests skip it.
async fn settle_quota(
    services: &RuntimeServices,
    grant: Option<&db::QuotaGrant>,
    request_id: uuid::Uuid,
    status: u16,
) {
    let Some(grant) = grant else {
        return;
    };
    let Some(state) = services.mcp_state() else {
        return;
    };
    let commit = (200..300).contains(&status);
    let Some(pool) = state.storage.postgres_pool() else {
        return;
    };
    // The provider-reported actual cost (Firecrawl `creditsUsed`) replaces the
    // reserved default when present; a missing/zero value keeps the
    // reservation and a failure releases it. The durable settlement is atomic
    // so the account is never charged the reservation and the actual cost
    // twice.
    let actual_units = if commit {
        crate::mcp::tracked_credits_used()
    } else {
        None
    };
    if let Err(err) =
        db::settle_reservation_with_actual(pool, request_id, commit, actual_units).await
    {
        tracing::warn!(
            error = %err,
            request_id = %request_id,
            "failed to settle MCP quota reservation"
        );
        return;
    }
    if let Some((slot, upstream_status)) = crate::mcp::tracked_upstream_failure()
        && matches!(upstream_status, 401 | 403 | 429)
        && slot == grant.credential.position as i16 + 1
    {
        let cooldown_seconds = if upstream_status == 429 { 60 } else { 600 };
        mcp::record_credential_failure(
            pool,
            &state.quota_valkey,
            &grant.credential,
            &format!("upstream http {upstream_status}"),
            Some(cooldown_seconds),
        )
        .await;
    }
}

#[cfg(test)]
mod stack_tests;
