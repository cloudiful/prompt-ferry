use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{McpCredential, McpQuotaGroup};

use super::{load_accounts_for_group, period};

/// Group usage ratio (used + reserved) / limit for the current month and day
/// windows. Returns the larger of the two ratios; a missing limit or account
/// contributes zero.
pub async fn group_usage_ratio(
    pool: &PgPool,
    group: &McpQuotaGroup,
    now: DateTime<Utc>,
) -> Result<f64> {
    let mut ratio = 0.0f64;
    if let Some(limit) = group.monthly_limit.filter(|limit| *limit > 0.0) {
        let period = period::current_month_period(group, now);
        let account = load_accounts_for_group(pool, group.group_id, "month", period.start)
            .await?
            .pop();
        if let Some(account) = account {
            ratio = ratio.max((account.used_units + account.reserved_units) / limit);
        }
    }
    if let Some(limit) = group.daily_limit.filter(|limit| *limit > 0.0) {
        let period = period::current_day_period(now);
        let account = load_accounts_for_group(pool, group.group_id, "day", period.start)
            .await?
            .pop();
        if let Some(account) = account {
            ratio = ratio.max((account.used_units + account.reserved_units) / limit);
        }
    }
    Ok(ratio)
}

/// Pick the credential with the lowest group usage ratio. Credentials without
/// a quota group are treated as unused (ratio 0) and tie-broken by position,
/// keeping the pre-quota least-used behaviour for unconfigured servers.
/// `skip` excludes credentials that were already tried and rejected.
pub async fn pick_credential(
    pool: &PgPool,
    credentials: &[McpCredential],
    now: DateTime<Utc>,
    skip: &[Uuid],
) -> Result<Option<McpCredential>> {
    let skip: std::collections::HashSet<Uuid> = skip.iter().copied().collect();
    let mut candidates = Vec::new();
    for credential in credentials {
        if skip.contains(&credential.credential_id)
            || !credential.enabled
            || credential.is_in_cooldown(now)
            || credential.is_exhausted()
            || credential.secret.trim().is_empty()
        {
            continue;
        }
        let ratio = match credential.quota_group_id {
            Some(group_id) => {
                let group = crate::db::get_quota_group(pool, group_id)
                    .await
                    .context("failed to load credential quota group")?;
                match group {
                    Some(group) => group_usage_ratio(pool, &group, now).await?,
                    None => 0.0,
                }
            }
            None => 0.0,
        };
        candidates.push((ratio, credential));
    }
    candidates.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.position.cmp(&right.1.position))
            .then_with(|| left.1.credential_id.cmp(&right.1.credential_id))
    });
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, credential)| credential.clone()))
}
