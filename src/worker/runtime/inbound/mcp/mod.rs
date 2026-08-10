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
    let Some(state) = services.admin_state() else {
        return;
    };
    let commit = (200..300).contains(&status);
    if let Err(err) = db::settle_reservation(&state.pool, request_id, commit).await {
        tracing::warn!(
            error = %err,
            request_id = %request_id,
            "failed to settle MCP quota reservation"
        );
        return;
    }
    if commit
        && let Some(credits_used) = crate::mcp::tracked_credits_used()
        && credits_used > grant.reservation.units
    {
        let extra = credits_used - grant.reservation.units;
        for account in [grant.day_account.as_ref(), grant.month_account.as_ref()]
            .into_iter()
            .flatten()
        {
            if let Err(err) = db::charge_extra_units(&state.pool, account.account_id, extra).await {
                tracing::warn!(
                    error = %err,
                    request_id = %request_id,
                    account_id = account.account_id,
                    "failed to charge extra MCP quota units"
                );
            }
        }
    }
    if let Some((slot, upstream_status)) = crate::mcp::tracked_upstream_failure()
        && matches!(upstream_status, 401 | 403 | 429)
        && slot == grant.credential.position as i16 + 1
    {
        let cooldown_seconds = if upstream_status == 429 { 60 } else { 600 };
        mcp::record_credential_failure(
            &state.pool,
            &state.mcp_quota_valkey,
            &grant.credential,
            &format!("upstream http {upstream_status}"),
            Some(cooldown_seconds),
        )
        .await;
    }
}

#[cfg(test)]
mod stack_tests;
