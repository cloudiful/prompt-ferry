pub mod period;
pub mod select;

pub use select::{group_usage_ratio, pick_credential};

use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{
    McpCredential, McpQuotaAccountRow, McpQuotaAccountSnapshot, QuotaGrant, QuotaPeriod,
    QuotaPeriodKind, QuotaReservation,
};

use self::period::{current_day_period, current_month_period};

const RESERVATION_TTL: ChronoDuration = ChronoDuration::minutes(5);

pub async fn load_accounts_for_group(
    pool: &PgPool,
    group_id: Uuid,
    period_kind: &str,
    period_start: DateTime<Utc>,
) -> Result<Vec<McpQuotaAccountSnapshot>> {
    let rows = sqlx::query_file_as!(
        McpQuotaAccountRow,
        "src/sql/quota/load_accounts_for_group.sql",
        group_id,
        period_kind,
        period_start,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| McpQuotaAccountSnapshot {
            account_id: row.account_id,
            period: QuotaPeriod {
                kind: if row.period_kind == "day" {
                    QuotaPeriodKind::Day
                } else {
                    QuotaPeriodKind::Month
                },
                start: row.period_start,
                end: row.period_end,
            },
            used_units: row.used_units,
            reserved_units: row.reserved_units,
        })
        .collect())
}

/// Atomically reserve quota for a request, choosing a credential up front.
///
/// Reservations are persisted inside a single transaction so concurrent
/// requests can never overspend a group.
#[derive(Debug, Clone)]
pub enum ReserveOutcome {
    Granted(Box<QuotaGrant>),
    /// No quota constraints configured; the request may proceed without a
    /// reservation.
    NoBudget,
    /// The group budget is exhausted; the request must be rejected.
    BudgetExceeded,
}

pub async fn reserve_for_credential(
    pool: &PgPool,
    credential: &McpCredential,
    request_id: Uuid,
    now: DateTime<Utc>,
) -> Result<ReserveOutcome> {
    let Some(group_id) = credential.quota_group_id else {
        return Ok(ReserveOutcome::NoBudget);
    };
    let Some(group) = crate::db::get_quota_group(pool, group_id).await? else {
        return Ok(ReserveOutcome::NoBudget);
    };
    let cost = credential.default_cost.max(0.0);
    let daily_limit = group.daily_limit;
    let monthly_limit = group.monthly_limit;
    if cost == 0.0 || (daily_limit.is_none() && monthly_limit.is_none()) {
        return Ok(ReserveOutcome::NoBudget);
    }

    let month_period = current_month_period(&group, now);
    let day_period = current_day_period(now);
    let mut tx = pool.begin().await?;

    let day_account = if daily_limit.is_some() {
        match reserve_account(&mut tx, group_id, day_period, cost, daily_limit).await? {
            Some(account) => Some(account),
            None => {
                tx.rollback().await?;
                return Ok(ReserveOutcome::BudgetExceeded);
            }
        }
    } else {
        None
    };

    let month_account = if monthly_limit.is_some() {
        match reserve_account(&mut tx, group_id, month_period, cost, monthly_limit).await? {
            Some(account) => Some(account),
            None => {
                tx.rollback().await?;
                return Ok(ReserveOutcome::BudgetExceeded);
            }
        }
    } else {
        None
    };

    let reservation = insert_reservation(
        &mut tx,
        day_account.as_ref(),
        month_account.as_ref(),
        credential.credential_id,
        request_id,
        cost,
        now + RESERVATION_TTL,
    )
    .await?;
    tx.commit().await?;
    Ok(ReserveOutcome::Granted(Box::new(QuotaGrant {
        credential: credential.clone(),
        reservation,
        day_account,
        month_account,
    })))
}

async fn reserve_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    group_id: Uuid,
    period: QuotaPeriod,
    cost: f64,
    limit: Option<f64>,
) -> Result<Option<McpQuotaAccountSnapshot>> {
    sqlx::query_file!(
        "src/sql/quota/ensure_account.sql",
        group_id,
        period.kind.as_str(),
        period.start,
        period.end,
    )
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query_file!(
        "src/sql/quota/reserve_units.sql",
        group_id,
        period.kind.as_str(),
        period.start,
        cost,
        limit,
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|row| McpQuotaAccountSnapshot {
        account_id: row.account_id,
        period: QuotaPeriod {
            kind: if row.period_kind == "day" {
                QuotaPeriodKind::Day
            } else {
                QuotaPeriodKind::Month
            },
            start: row.period_start,
            end: row.period_end,
        },
        used_units: row.used_units,
        reserved_units: row.reserved_units,
    }))
}

async fn insert_reservation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    day_account: Option<&McpQuotaAccountSnapshot>,
    month_account: Option<&McpQuotaAccountSnapshot>,
    credential_id: Uuid,
    request_id: Uuid,
    units: f64,
    expires_at: DateTime<Utc>,
) -> Result<QuotaReservation> {
    let row = sqlx::query_file!(
        "src/sql/quota/insert_reservation.sql",
        day_account.map(|account| account.account_id),
        month_account.map(|account| account.account_id),
        credential_id,
        request_id,
        units,
        expires_at,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(QuotaReservation {
        reservation_id: row.reservation_id,
        account_id: row
            .month_account_id
            .or(row.day_account_id)
            .unwrap_or_default(),
        credential_id,
        request_id,
        units: row.units,
    })
}

/// Settle a reservation: move reserved units into used on success, or release
/// them back on failure. Returns `false` when no active reservation existed.
pub async fn settle_reservation(pool: &PgPool, request_id: Uuid, commit: bool) -> Result<bool> {
    let status = if commit { "committed" } else { "released" };
    let mut tx = pool.begin().await?;
    let row = sqlx::query_file!(
        "src/sql/quota/settle_reservation_status.sql",
        request_id,
        status,
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(false);
    };
    let units = row.units;
    if let Some(account_id) = row.day_account_id {
        settle_account(&mut tx, account_id, units, commit).await?;
    }
    if let Some(account_id) = row.month_account_id {
        settle_account(&mut tx, account_id, units, commit).await?;
    }
    tx.commit().await?;
    Ok(true)
}

async fn settle_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: i64,
    units: f64,
    commit: bool,
) -> Result<()> {
    if commit {
        sqlx::query_file!("src/sql/quota/settle_commit.sql", account_id, units)
            .fetch_optional(&mut **tx)
            .await?;
    } else {
        sqlx::query_file!("src/sql/quota/settle_release.sql", account_id, units)
            .fetch_optional(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Release reservations whose TTL expired (worker crash or lost completion).
pub async fn release_expired_reservations(pool: &PgPool) -> Result<i64> {
    let result = sqlx::query_file!("src/sql/quota/release_expired_reservations.sql")
        .execute(pool)
        .await?;
    Ok(result.rows_affected() as i64)
}

/// Charge additional units to an account after the real provider cost is
/// known (e.g. Firecrawl `creditsUsed` exceeds the reserved default cost).
pub async fn charge_extra_units(pool: &PgPool, account_id: i64, units: f64) -> Result<()> {
    if units <= 0.0 {
        return Ok(());
    }
    sqlx::query_file!("src/sql/quota/charge_extra_units.sql", account_id, units)
        .fetch_optional(pool)
        .await?;
    Ok(())
}

pub async fn mark_credential_failure(
    pool: &PgPool,
    credential_id: Uuid,
    cooldown_seconds: Option<u64>,
    error: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    let cooldown_until =
        cooldown_seconds.map(|seconds| now + ChronoDuration::seconds(seconds as i64));
    sqlx::query_file!(
        "src/sql/quota/set_credential_failure.sql",
        credential_id,
        cooldown_until,
        error,
        now,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_credential_provider_remaining(
    pool: &PgPool,
    credential_id: Uuid,
    remaining: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/quota/set_credential_provider_remaining.sql",
        credential_id,
        remaining,
        now,
        reset_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}
