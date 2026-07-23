use super::*;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{Acquire, postgres::PgConnection};
use std::collections::HashSet;

const RAW_PARTITION_HORIZON_DAYS: i64 = 7;
const RAW_PARTITION_PREFIX: &str = "request_record_raw_payloads_";

#[derive(Debug, sqlx::FromRow)]
struct RawPartitionRow {
    name: String,
}

pub async fn ensure_raw_payload_partitions(
    connection: &mut PgConnection,
    now: DateTime<Utc>,
) -> Result<u64> {
    let mut transaction = connection.begin().await?;
    // Prevent concurrent inserts routed to DEFAULT while rows are staged and
    // the matching partition is created.
    sqlx::query_file!("src/sql/usage/lock_default_raw_payloads.sql")
        .execute(&mut *transaction)
        .await?;
    let existing_partitions = sqlx::query_file_as!(
        RawPartitionRow,
        "src/sql/usage/list_raw_payload_partitions.sql"
    )
    .fetch_all(&mut *transaction)
    .await?
    .into_iter()
    .map(|partition| partition.name)
    .collect::<HashSet<_>>();
    let today = now.date_naive();
    let mut created = 0;

    for offset in 0..=RAW_PARTITION_HORIZON_DAYS {
        let start = today
            .checked_add_days(chrono::Days::new(offset as u64))
            .ok_or_else(|| anyhow::anyhow!("raw partition date overflow"))?;
        let end = start
            .checked_add_days(chrono::Days::new(1))
            .ok_or_else(|| anyhow::anyhow!("raw partition date overflow"))?;
        let start_at = utc_midnight(start)?;
        let end_at = utc_midnight(end)?;
        let partition_name = raw_partition_name(start);

        sqlx::query_file!(
            "src/sql/usage/stage_default_raw_payloads.sql",
            start_at,
            end_at,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query_file!(
            "src/sql/usage/delete_default_raw_payloads.sql",
            start_at,
            end_at,
        )
        .execute(&mut *transaction)
        .await?;

        // Partition identifiers and bounds are generated exclusively from a
        // validated NaiveDate; PostgreSQL parameters cannot bind identifiers.
        let ddl = format!(
            "CREATE TABLE IF NOT EXISTS {partition_name} PARTITION OF request_record_raw_payloads FOR VALUES FROM ('{}') TO ('{}')",
            start_at.to_rfc3339(),
            end_at.to_rfc3339(),
        );
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *transaction)
            .await?;
        if !existing_partitions.contains(&partition_name) {
            created += 1;
        }

        sqlx::query_file!("src/sql/usage/insert_staged_raw_payloads.sql")
            .execute(&mut *transaction)
            .await?;
        sqlx::query_file!("src/sql/usage/delete_staged_raw_payloads.sql")
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    Ok(created)
}

pub async fn drop_expired_raw_payload_partitions(
    connection: &mut PgConnection,
    cutoff: DateTime<Utc>,
) -> Result<u64> {
    let partitions = sqlx::query_file_as!(
        RawPartitionRow,
        "src/sql/usage/list_raw_payload_partitions.sql"
    )
    .fetch_all(&mut *connection)
    .await?;
    let mut dropped = 0;

    for partition in partitions {
        let Some(day) = partition.name.strip_prefix(RAW_PARTITION_PREFIX) else {
            continue;
        };
        let Ok(day) = NaiveDate::parse_from_str(day, "%Y%m%d") else {
            continue;
        };
        let end_at = utc_midnight(
            day.checked_add_days(chrono::Days::new(1))
                .ok_or_else(|| anyhow::anyhow!("raw partition date overflow"))?,
        )?;
        if end_at > cutoff {
            continue;
        }

        // Names come from the controlled catalog query and are validated above.
        let ddl = format!("DROP TABLE IF EXISTS {}", partition.name);
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *connection)
            .await?;
        dropped += 1;
    }

    Ok(dropped)
}

fn raw_partition_name(day: NaiveDate) -> String {
    format!("{RAW_PARTITION_PREFIX}{}", day.format("%Y%m%d"))
}

fn utc_midnight(day: NaiveDate) -> Result<DateTime<Utc>> {
    day.and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
        .ok_or_else(|| anyhow::anyhow!("invalid raw partition date"))
}
