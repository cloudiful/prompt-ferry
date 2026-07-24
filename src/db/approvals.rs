use anyhow::Result;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::llm_review::ApprovalStatus;

use super::types::{
    ApprovalRequest, ApprovalRequestCreate, ApprovalRequestPage, ApprovalStatusFilter,
    FlaggedApprovalRequestInput,
};

pub async fn create_approval_request(
    pool: &PgPool,
    create: ApprovalRequestCreate,
) -> Result<ApprovalRequest> {
    sqlx::query_file_as!(
        ApprovalRequest,
        "src/sql/approvals/create_approval_request.sql",
        create.approval_id,
        create.request_id,
        create.user_id,
        create.client_key_label,
        create.path,
        create.model,
        create.review_decision,
        create.approval_status,
        create.review_reason,
        &create.review_categories,
        create.request_preview,
        create.request_payload_json,
        create.request_deadline_unix_ms,
        create.wait_deadline_unix_ms,
    )
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_approval_requests_page(
    pool: &PgPool,
    status: ApprovalStatusFilter,
    first: i64,
    rows: i64,
) -> Result<ApprovalRequestPage> {
    let first = first.max(0);
    let rows = rows.clamp(1, 100);
    let (total, approvals) = match status {
        ApprovalStatusFilter::Pending => {
            let total = sqlx::query_file!("src/sql/approvals/count_pending_approval_requests.sql")
                .fetch_one(pool)
                .await?
                .total;
            let approvals = sqlx::query_file_as!(
                ApprovalRequest,
                "src/sql/approvals/list_pending_approval_requests_page.sql",
                first,
                rows,
            )
            .fetch_all(pool)
            .await?;
            (total, approvals)
        }
        ApprovalStatusFilter::Resolved => {
            let total = sqlx::query_file!("src/sql/approvals/count_resolved_approval_requests.sql")
                .fetch_one(pool)
                .await?
                .total;
            let approvals = sqlx::query_file_as!(
                ApprovalRequest,
                "src/sql/approvals/list_resolved_approval_requests_page.sql",
                first,
                rows,
            )
            .fetch_all(pool)
            .await?;
            (total, approvals)
        }
    };
    Ok(ApprovalRequestPage {
        total,
        approvals,
        first,
        rows,
    })
}

pub async fn get_approval_request(
    pool: &PgPool,
    approval_id: Uuid,
) -> Result<Option<ApprovalRequest>> {
    sqlx::query_file_as!(
        ApprovalRequest,
        "src/sql/approvals/get_approval_request.sql",
        approval_id,
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub async fn resolve_approval_request(
    pool: &PgPool,
    approval_id: Uuid,
    status: ApprovalStatus,
    decided_by_user_id: Option<i64>,
) -> Result<Option<ApprovalRequest>> {
    sqlx::query_file_as!(
        ApprovalRequest,
        "src/sql/approvals/resolve_approval_request.sql",
        approval_id,
        status.as_str(),
        decided_by_user_id,
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub async fn abort_pending_approval_requests(pool: &PgPool) -> Result<u64> {
    let result = sqlx::query_file!("src/sql/approvals/abort_pending_approval_requests.sql")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn record_approval_webhook_result(
    pool: &PgPool,
    approval_id: Uuid,
    error: Option<String>,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/approvals/record_approval_webhook_result.sql",
        approval_id,
        error,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_flagged_approval_request(
    pool: &PgPool,
    input: FlaggedApprovalRequestInput,
) -> Result<ApprovalRequest> {
    create_approval_request(pool, ApprovalRequestCreate::flagged(input)).await
}

pub async fn approval_request_status(
    pool: &PgPool,
    approval_id: Uuid,
) -> Result<Option<(String, Option<chrono::DateTime<Utc>>)>> {
    Ok(
        sqlx::query_file!("src/sql/approvals/approval_request_status.sql", approval_id,)
            .fetch_optional(pool)
            .await?
            .map(|row| (row.approval_status, row.decided_at)),
    )
}
