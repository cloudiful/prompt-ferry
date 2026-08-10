mod valkey;

pub use valkey::McpQuotaValkey;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::{McpCredential, QuotaGrant};

const MAX_PICK_ATTEMPTS: usize = 3;

/// Outcome of preparing quota for one MCP request.
pub enum QuotaDecision {
    /// Quota reserved and a credential selected for the upstream call.
    Granted { grant: Box<QuotaGrant> },
    /// No quota constraints configured; fall back to legacy token selection.
    Unconstrained,
    /// Every credential is exhausted, cooling down, or disabled.
    Exhausted,
    /// Internal failure; reject rather than spend unbudgeted quota.
    Unavailable { reason: String },
}

/// Select a credential for the server and atomically reserve its budget.
///
/// Retries with the next best credential when a concurrent request claimed
/// the budget of the first pick.
pub async fn prepare_quota(
    pool: &sqlx::PgPool,
    server_id: Uuid,
    request_id: Uuid,
    now: DateTime<Utc>,
) -> QuotaDecision {
    let credentials = match crate::db::list_credentials_by_server(pool, server_id).await {
        Ok(credentials) => credentials,
        Err(err) => {
            return QuotaDecision::Unavailable {
                reason: err.to_string(),
            };
        }
    };
    if credentials.is_empty() {
        return QuotaDecision::Unconstrained;
    }

    let mut skipped = Vec::new();
    for _ in 0..MAX_PICK_ATTEMPTS {
        let picked = match crate::db::pick_credential(pool, &credentials, now, &skipped).await {
            Ok(picked) => picked,
            Err(err) => {
                return QuotaDecision::Unavailable {
                    reason: err.to_string(),
                };
            }
        };
        let Some(credential) = picked else {
            return QuotaDecision::Exhausted;
        };
        match crate::db::reserve_for_credential(pool, &credential, request_id, now).await {
            Ok(crate::db::ReserveOutcome::Granted(grant)) => {
                return QuotaDecision::Granted { grant };
            }
            Ok(crate::db::ReserveOutcome::NoBudget) => return QuotaDecision::Unconstrained,
            Ok(crate::db::ReserveOutcome::BudgetExceeded) => {
                skipped.push(credential.credential_id);
            }
            Err(err) => {
                return QuotaDecision::Unavailable {
                    reason: err.to_string(),
                };
            }
        }
    }
    QuotaDecision::Exhausted
}

/// Mark a credential as failed after an upstream 401/403/429, applying a
/// cooldown so the balancer skips it. `None` is written for the remaining
/// balance when the provider reports one.
pub async fn record_credential_failure(
    pool: &sqlx::PgPool,
    valkey: &McpQuotaValkey,
    credential: &McpCredential,
    error: &str,
    cooldown_seconds: Option<u64>,
) {
    let now = Utc::now();
    if let Err(err) = crate::db::mark_credential_failure(
        pool,
        credential.credential_id,
        cooldown_seconds,
        error,
        now,
    )
    .await
    {
        tracing::warn!(
            error = %err,
            credential_id = %credential.credential_id,
            "failed to persist MCP credential failure"
        );
    }
    if let Some(until) =
        cooldown_seconds.map(|seconds| now + chrono::Duration::seconds(seconds as i64))
    {
        valkey.set_cooldown(credential.credential_id, until).await;
    }
}
