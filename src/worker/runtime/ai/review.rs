use super::super::{
    RequestExecutionContext, RequestPromptLog,
    context::{FailurePayload, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
};
use super::errors::respond_with_local_error;
use crate::{
    db,
    llm_review::{
        self, ApprovalResolution, ApprovalStatus, ReviewDecision, ReviewFailure,
        compute_wait_deadline_unix_ms, request_payload_json, review_request,
        spawn_approval_webhook,
    },
    usage::{extract_request_prompt, truncate_chars},
    worker_admin::AdminState,
};
use anyhow::anyhow;
use chrono::Utc;
use reqwest::{Client, StatusCode};
use std::time::Duration;

pub(super) async fn handle_llm_review_gate(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
) -> anyhow::Result<bool> {
    let state = services
        .admin_state()
        .ok_or_else(|| anyhow!("admin state unavailable for review gate"))?;
    if request.path != "/v1/chat/completions" && request.path != "/v1/responses" {
        return Ok(true);
    }

    let settings = state.llm_review_settings.read().await.clone();
    if !settings.enabled {
        return Ok(true);
    }

    let request_preview = request_preview_for_review(request, &request_ctx.request_prompt_log);
    let payload_json = request_payload_json(&request.body);
    let review = review_request(
        &services.client,
        &settings,
        llm_review::ReviewRequest {
            path: &request.path,
            model: request_ctx.request_model.as_deref(),
            request_preview: &request_preview,
            request_payload_json: &payload_json,
        },
    )
    .await;

    match review {
        Ok(result) if result.decision == ReviewDecision::Allow => Ok(true),
        Ok(result) if result.decision == ReviewDecision::Flag => {
            let now_unix_ms = Utc::now().timestamp_millis();
            let wait_deadline_unix_ms = compute_wait_deadline_unix_ms(
                now_unix_ms,
                request.request_deadline_unix_ms,
                settings.review_timeout_ms,
                settings.pending_ttl_seconds,
            );
            let approval = db::create_flagged_approval_request(
                &state.pool,
                db::FlaggedApprovalRequestInput {
                    request_id: request_ctx.request_id,
                    user_id: request_ctx.user_id,
                    client_key_label: request_ctx.client_key_label.clone(),
                    path: request.path.clone(),
                    model: request_ctx.request_model.clone(),
                    review_reason: result.reason.clone(),
                    review_categories: result.categories.clone(),
                    request_preview,
                    request_payload_json: payload_json,
                    request_deadline_unix_ms: request.request_deadline_unix_ms,
                    wait_deadline_unix_ms,
                },
            )
            .await?;
            let _ = db::record_request_state(
                &state.pool,
                db::RequestRecordStateInput {
                    request_id: request_ctx.request_id,
                    request_state: db::RequestRecordState::AwaitingApproval,
                    endpoint_id: None,
                    model_route_rule_id: None,
                    model: request_ctx.request_model.as_deref(),
                    endpoint_key_id: None,
                    endpoint_key_label: None,
                },
            )
            .await;
            spawn_approval_webhook(
                state.pool.clone(),
                services.client.clone(),
                settings.clone(),
                "approval.pending",
                approval.clone(),
            );
            let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
            state
                .approval_waiters
                .lock()
                .await
                .insert(approval.approval_id, wait_tx);
            if services
                .out_tx
                .send(crate::protocol::BridgeMessage::ApprovalPending(
                    crate::protocol::ApprovalPending {
                        request_id: request.request_id.clone(),
                    },
                ))
                .await
                .is_err()
            {
                state
                    .approval_waiters
                    .lock()
                    .await
                    .remove(&approval.approval_id);
                let _ = db::resolve_approval_request(
                    &state.pool,
                    approval.approval_id,
                    ApprovalStatus::Aborted,
                    None,
                )
                .await;
                return Err(anyhow!("relay response channel closed"));
            }

            let remaining_ms = wait_deadline_unix_ms.saturating_sub(Utc::now().timestamp_millis());
            let wait_result = if remaining_ms <= 0 {
                ApprovalResolution::Expired
            } else {
                match tokio::time::timeout(Duration::from_millis(remaining_ms as u64), wait_rx)
                    .await
                {
                    Ok(Ok(resolution)) => resolution,
                    Ok(Err(_)) => ApprovalResolution::Interrupted,
                    Err(_) => ApprovalResolution::Expired,
                }
            };

            match wait_result {
                ApprovalResolution::Approved => Ok(true),
                ApprovalResolution::Rejected => {
                    respond_with_local_error(
                        services,
                        request,
                        request_ctx,
                        FailurePayload {
                            status: StatusCode::FORBIDDEN,
                            error_code: "approval_rejected".to_string(),
                            error_message: "request was rejected during manual approval"
                                .to_string(),
                            upstream_error_body: None,
                            response_body: None,
                        },
                    )
                    .await?;
                    Ok(false)
                }
                ApprovalResolution::Expired => {
                    state
                        .approval_waiters
                        .lock()
                        .await
                        .remove(&approval.approval_id);
                    if let Some(expired) = db::resolve_approval_request(
                        &state.pool,
                        approval.approval_id,
                        ApprovalStatus::Expired,
                        None,
                    )
                    .await?
                    {
                        spawn_approval_webhook(
                            state.pool.clone(),
                            services.client.clone(),
                            settings,
                            "approval.expired",
                            expired,
                        );
                    }
                    respond_with_local_error(
                        services,
                        request,
                        request_ctx,
                        FailurePayload {
                            status: StatusCode::FORBIDDEN,
                            error_code: "approval_expired".to_string(),
                            error_message: "approval wait expired before a decision was made"
                                .to_string(),
                            upstream_error_body: None,
                            response_body: None,
                        },
                    )
                    .await?;
                    Ok(false)
                }
                ApprovalResolution::Interrupted => {
                    state
                        .approval_waiters
                        .lock()
                        .await
                        .remove(&approval.approval_id);
                    respond_with_local_error(
                        services,
                        request,
                        request_ctx,
                        FailurePayload {
                            status: StatusCode::SERVICE_UNAVAILABLE,
                            error_code: "approval_interrupted".to_string(),
                            error_message: "approval wait was interrupted".to_string(),
                            upstream_error_body: None,
                            response_body: None,
                        },
                    )
                    .await?;
                    Ok(false)
                }
            }
        }
        Ok(_) => Ok(true),
        Err(err) => match settings.failure_policy {
            llm_review::ReviewFailurePolicy::FailOpen => Ok(true),
            llm_review::ReviewFailurePolicy::FailClosed => {
                let message = match err {
                    ReviewFailure::Timeout => "review request timed out",
                    ReviewFailure::Error(_) => "review request failed",
                };
                respond_with_local_error(
                    services,
                    request,
                    request_ctx,
                    FailurePayload {
                        status: StatusCode::SERVICE_UNAVAILABLE,
                        error_code: "review_unavailable".to_string(),
                        error_message: message.to_string(),
                        upstream_error_body: None,
                        response_body: None,
                    },
                )
                .await?;
                Ok(false)
            }
        },
    }
}

pub(in crate::worker::runtime) async fn abort_waiting_approvals(
    state: &AdminState,
    client: &Client,
) {
    let waiter_ids = state
        .approval_waiters
        .lock()
        .await
        .drain()
        .map(|(approval_id, sender)| {
            let _ = sender.send(ApprovalResolution::Interrupted);
            approval_id
        })
        .collect::<Vec<_>>();
    if waiter_ids.is_empty() {
        return;
    }
    let settings = state.llm_review_settings.read().await.clone();
    for approval_id in waiter_ids {
        match db::resolve_approval_request(&state.pool, approval_id, ApprovalStatus::Aborted, None)
            .await
        {
            Ok(Some(approval)) => {
                spawn_approval_webhook(
                    state.pool.clone(),
                    client.clone(),
                    settings.clone(),
                    "approval.aborted",
                    approval,
                );
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    approval_id = %approval_id,
                    error = %err,
                    "failed to abort waiting approval"
                )
            }
        }
    }
}

fn request_preview_for_review(
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> String {
    let _ = request_prompt_log;
    extract_request_prompt(&request.path, &request.body)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| truncate_chars(&String::from_utf8_lossy(&request.body), 2_000))
}
