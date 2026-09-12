mod firecrawl;

pub use firecrawl::{FirecrawlBalanceClient, ProviderBalance, ProviderBalanceError};

use chrono::Utc;
use futures::{StreamExt, stream};
use sqlx::PgPool;
use tracing::warn;

use crate::db::McpProviderSecret;

use super::quota::McpQuotaValkey;

/// Maximum number of provider balance lookups in flight. The refresh loop
/// must never fan out unbounded network calls against a provider API.
pub const FIRECRAWL_BALANCE_REFRESH_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FirecrawlRefreshSummary {
    pub attempted: usize,
    pub refreshed: usize,
    pub failed: usize,
}

/// Fetch and persist one Firecrawl credential's provider balance.
///
/// Shared by the scheduled loop and the admin refresh action so both paths use
/// the same durable write and Valkey cache update. A successful response is
/// persisted before returning; a failed response keeps the previous durable
/// balance, records only a sanitized `last_error`, and returns the sanitized
/// error. `Persistence` signals that the provider call succeeded but the
/// snapshot could not be written.
pub async fn refresh_firecrawl_credential(
    pool: &PgPool,
    valkey: &McpQuotaValkey,
    client: &FirecrawlBalanceClient,
    credential: &McpProviderSecret,
) -> Result<ProviderBalance, ProviderBalanceError> {
    match client.fetch_balance(&credential.secret).await {
        Ok(balance) => {
            let synced_at = Utc::now();
            if let Err(err) = crate::db::update_credential_provider_remaining(
                pool,
                credential.credential_id,
                Some(balance.remaining),
                balance.reset_at,
                synced_at,
            )
            .await
            {
                warn!(
                    error = %err,
                    credential_id = %credential.credential_id,
                    "failed to persist Firecrawl provider balance"
                );
                return Err(ProviderBalanceError::Persistence);
            }
            valkey
                .set_provider_remaining(
                    credential.credential_id,
                    balance.remaining,
                    balance.reset_at,
                )
                .await;
            Ok(balance)
        }
        Err(err) => {
            if let Err(record_err) = crate::db::record_credential_provider_sync_error(
                pool,
                credential.credential_id,
                &err.to_string(),
                Utc::now(),
            )
            .await
            {
                warn!(
                    error = %record_err,
                    credential_id = %credential.credential_id,
                    "failed to record Firecrawl provider sync error"
                );
            }
            warn!(
                error = %err,
                credential_id = %credential.credential_id,
                "Firecrawl provider balance refresh failed"
            );
            Err(err)
        }
    }
}

/// Refresh the durable provider balance for every enabled Firecrawl
/// credential. `provider_kind = firecrawl` is the eligibility gate, so legacy
/// NULL/generic credentials never trigger a Firecrawl call.
///
/// Failed lookups keep the previous durable balance and only record a
/// sanitized `last_error`; they never write a fabricated zero.
pub async fn refresh_firecrawl_balances(
    pool: &PgPool,
    valkey: &McpQuotaValkey,
    client: &FirecrawlBalanceClient,
) -> anyhow::Result<FirecrawlRefreshSummary> {
    // The owning MCP server is the source of truth for a credential's
    // provider; reconcile pre-existing rows before selecting targets.
    crate::db::backfill_credential_provider_kinds(pool).await?;
    let credentials = crate::db::list_firecrawl_credentials(pool).await?;
    let attempted = credentials.len();
    let outcomes = map_bounded(
        credentials,
        FIRECRAWL_BALANCE_REFRESH_CONCURRENCY,
        |credential| async move {
            refresh_firecrawl_credential(pool, valkey, client, &credential)
                .await
                .is_ok()
        },
    )
    .await;
    let refreshed = outcomes.iter().filter(|ok| **ok).count();
    Ok(FirecrawlRefreshSummary {
        attempted,
        refreshed,
        failed: attempted - refreshed,
    })
}

/// Run `f` over `items` with at most `limit` concurrent futures, preserving
/// input order in the output.
async fn map_bounded<T, R, F, Fut>(items: Vec<T>, limit: usize, f: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> Fut,
    Fut: std::future::Future<Output = R> + Send,
{
    stream::iter(items)
        .map(f)
        .buffered(limit.max(1))
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn bounded_mapping_respects_the_concurrency_limit() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let items: Vec<usize> = (0..12).collect();
        let (active_handle, peak_handle) = (active.clone(), peak.clone());
        let results = map_bounded(items, FIRECRAWL_BALANCE_REFRESH_CONCURRENCY, move |item| {
            let active = active_handle.clone();
            let peak = peak_handle.clone();
            async move {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                item * 2
            }
        })
        .await;
        assert!(
            peak.load(Ordering::SeqCst) <= FIRECRAWL_BALANCE_REFRESH_CONCURRENCY,
            "peak concurrency {} exceeded the limit",
            peak.load(Ordering::SeqCst)
        );
        assert_eq!(results, (0..12).map(|item| item * 2).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn bounded_mapping_keeps_empty_input_empty() {
        let results = map_bounded(Vec::<usize>::new(), 4, |item| async move { item }).await;
        assert!(results.is_empty());
    }
}
