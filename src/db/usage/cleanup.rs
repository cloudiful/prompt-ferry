use super::*;
use crate::db::partition_maintenance::{
    RAW_PAYLOAD_PARENT, chunk_retention_cutoff, finish_with_lock, try_acquire_partition_lock,
};
use chrono::Duration as ChronoDuration;
use sqlx::{Connection, Postgres, Transaction, pool::PoolConnection};
use std::time::Duration;

const RAW_REQUEST_PRUNE_BATCH_SIZE: i64 = 500;
const RAW_REQUEST_PRUNE_LOCK_KEY: i64 = 0x7072_756e_6552_6177;

#[derive(Debug)]
struct RawPayloadPruneBatch {
    deleted_count: Option<i64>,
    cleared_count: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawPayloadMaintenanceReport {
    pub chunks_dropped: u64,
    pub raw_rows_deleted: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawChunkBranch {
    FullDrop,
    Boundary,
}

#[derive(Debug)]
struct ExpiredRawChunk {
    chunk_schema: String,
    chunk_name: String,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    lock_sql: String,
}

#[derive(Debug)]
struct PairedMetadataChunk {
    chunk_schema: String,
    chunk_name: String,
    lock_sql: String,
}

pub async fn run_raw_payload_maintenance(
    pool: &PgPool,
    retention_days: i64,
) -> Result<Option<RawPayloadMaintenanceReport>> {
    // Raw maintenance is serialized against clear and the shared chunk tick
    // by the same global key, then its own raw key: global first, raw second.
    let Some(mut conn) = try_acquire_partition_lock(pool).await? else {
        return Ok(None);
    };
    let raw_acquired = sqlx::query_file_scalar!(
        "src/sql/usage/try_acquire_raw_request_prune_lock.sql",
        RAW_REQUEST_PRUNE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await?
    .unwrap_or(false);
    if !raw_acquired {
        return finish_with_lock(&mut conn, Ok(None)).await;
    }

    let result = run_raw_payload_maintenance_locked(&mut conn, retention_days).await;
    match release_raw_request_prune_lock(&mut conn).await {
        Ok(true) => finish_with_lock(&mut conn, result).await.map(Some),
        Ok(false) => {
            // Closing the session releases both advisory locks.
            conn.close_on_drop();
            Err(anyhow::anyhow!(
                "raw request prune advisory lock was not held during release"
            ))
        }
        Err(unlock_error) => {
            tracing::error!(
                error = %unlock_error,
                "failed to release raw request prune advisory lock; closing connection"
            );
            conn.close_on_drop();
            result.and(Err(unlock_error))
        }
    }
}

async fn run_raw_payload_maintenance_locked(
    conn: &mut PoolConnection<Postgres>,
    retention_days: i64,
) -> Result<RawPayloadMaintenanceReport> {
    let now = Utc::now();
    let retention_days = retention_days.max(1);
    let prune_cutoff = now - ChronoDuration::days(retention_days);
    let drop_before = chunk_retention_cutoff(now, retention_days);
    let mut report = RawPayloadMaintenanceReport::default();

    let chunks = sqlx::query_file!("src/sql/usage/expired_raw_chunks.sql", prune_cutoff)
        .fetch_all(&mut **conn)
        .await?;
    for chunk in chunks.into_iter().map(|row| ExpiredRawChunk {
        chunk_schema: row.chunk_schema,
        chunk_name: row.chunk_name,
        range_start: row.range_start,
        range_end: row.range_end,
        lock_sql: row.lock_sql,
    }) {
        let Some(branch) = raw_chunk_branch(
            chunk.range_start,
            chunk.range_end,
            prune_cutoff,
            drop_before,
        ) else {
            continue;
        };

        let mut tx = (&mut **conn).begin().await?;
        sqlx::query(sqlx::AssertSqlSafe(chunk.lock_sql.as_str()))
            .execute(&mut *tx)
            .await?;

        if branch == RawChunkBranch::Boundary {
            decompress_chunk_if_needed(&mut tx, &chunk.chunk_schema, &chunk.chunk_name).await?;
        }

        let metadata = paired_metadata_chunk(&mut tx, &chunk).await?;
        if let Some(metadata) = &metadata {
            sqlx::query(sqlx::AssertSqlSafe(metadata.lock_sql.as_str()))
                .execute(&mut *tx)
                .await?;
            decompress_chunk_if_needed(&mut tx, &metadata.chunk_schema, &metadata.chunk_name)
                .await?;
        }

        match branch {
            RawChunkBranch::FullDrop => {
                if metadata.is_some() {
                    sqlx::query_file!(
                        "src/sql/usage/clear_expired_raw_payload_metadata.sql",
                        prune_cutoff,
                        chunk.range_start,
                        chunk.range_end,
                    )
                    .execute(&mut *tx)
                    .await?;
                }
                let dropped = sqlx::query_file!(
                    "src/sql/partitions/drop_one_chunk.sql",
                    RAW_PAYLOAD_PARENT,
                    chunk.range_end,
                    chunk.range_start,
                )
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|row| row.dropped)
                .collect::<Vec<_>>();
                verify_dropped_chunk(&dropped, &chunk.chunk_schema, &chunk.chunk_name)?;
                report.chunks_dropped += 1;
            }
            RawChunkBranch::Boundary => {
                report.raw_rows_deleted += prune_boundary_chunk(
                    &mut tx,
                    prune_cutoff,
                    chunk.range_start,
                    chunk.range_end,
                    metadata.is_some(),
                )
                .await?;
            }
        }

        tx.commit().await?;
    }

    Ok(report)
}

async fn paired_metadata_chunk(
    tx: &mut Transaction<'_, Postgres>,
    raw: &ExpiredRawChunk,
) -> Result<Option<PairedMetadataChunk>> {
    let rows = sqlx::query_file!(
        "src/sql/usage/paired_metadata_chunk.sql",
        raw.range_start,
        raw.range_end,
    )
    .fetch_all(&mut **tx)
    .await?;
    match rows.len() {
        0 => Ok(None),
        1 => {
            let row = rows.into_iter().next().expect("one row");
            Ok(Some(PairedMetadataChunk {
                chunk_schema: row.chunk_schema,
                chunk_name: row.chunk_name,
                lock_sql: row.lock_sql,
            }))
        }
        _ => Err(anyhow::anyhow!(
            "multiple metadata chunks share the raw chunk range {}..{}",
            raw.range_start,
            raw.range_end
        )),
    }
}

async fn decompress_chunk_if_needed(
    tx: &mut Transaction<'_, Postgres>,
    chunk_schema: &str,
    chunk_name: &str,
) -> Result<()> {
    let is_compressed = sqlx::query_file_scalar!(
        "src/sql/partitions/clear_chunk_compression_state.sql",
        chunk_schema,
        chunk_name,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        anyhow::anyhow!(
            "chunk {chunk_schema}.{chunk_name} compression state is missing; retry raw maintenance"
        )
    })?;
    if is_compressed {
        sqlx::query_file!(
            "src/sql/partitions/decompress_clear_chunk.sql",
            chunk_schema,
            chunk_name,
        )
        .fetch_one(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Delete the expired raw rows of one boundary chunk and clear the paired
/// metadata for exactly those rows. Every batch runs inside the same chunk
/// transaction, so a failure rolls back the deletes and both decompressions.
async fn prune_boundary_chunk(
    tx: &mut Transaction<'_, Postgres>,
    cutoff: DateTime<Utc>,
    chunk_start: DateTime<Utc>,
    chunk_end: DateTime<Utc>,
    metadata_present: bool,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let batch = sqlx::query_file_as!(
            RawPayloadPruneBatch,
            "src/sql/usage/prune_usage_raw_requests_batch.sql",
            cutoff,
            RAW_REQUEST_PRUNE_BATCH_SIZE,
            chunk_start,
            chunk_end,
        )
        .fetch_one(&mut **tx)
        .await?;
        let deleted_count = batch.deleted_count.unwrap_or_default();
        let cleared_count = batch.cleared_count.unwrap_or_default();
        total += deleted_count.max(0) as u64;
        if deleted_count == 0 {
            break;
        }
        if metadata_present && cleared_count != deleted_count {
            tracing::warn!(
                deleted = deleted_count,
                cleared = cleared_count,
                "raw payload cleanup deleted rows without clearing all main-record metadata"
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(total)
}

fn raw_chunk_branch(
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    prune_cutoff: DateTime<Utc>,
    drop_before: DateTime<Utc>,
) -> Option<RawChunkBranch> {
    if range_start >= prune_cutoff {
        return None;
    }
    if range_end <= drop_before {
        Some(RawChunkBranch::FullDrop)
    } else {
        Some(RawChunkBranch::Boundary)
    }
}

fn verify_dropped_chunk(dropped: &[String], chunk_schema: &str, chunk_name: &str) -> Result<()> {
    match dropped {
        [one] if dropped_chunk_matches(one, chunk_schema, chunk_name) => Ok(()),
        [] => Err(anyhow::anyhow!(
            "raw chunk drop returned no chunk for {chunk_schema}.{chunk_name}"
        )),
        [_] => Err(anyhow::anyhow!(
            "raw chunk drop returned a different chunk than {chunk_schema}.{chunk_name}"
        )),
        _ => Err(anyhow::anyhow!(
            "raw chunk drop returned {} chunks for {chunk_schema}.{chunk_name}",
            dropped.len()
        )),
    }
}

fn dropped_chunk_matches(dropped: &str, chunk_schema: &str, chunk_name: &str) -> bool {
    dropped == chunk_name || dropped == format!("{chunk_schema}.{chunk_name}")
}

async fn release_raw_request_prune_lock(conn: &mut PoolConnection<Postgres>) -> Result<bool> {
    let released = sqlx::query_file_scalar!(
        "src/sql/usage/release_raw_request_prune_lock.sql",
        RAW_REQUEST_PRUNE_LOCK_KEY
    )
    .fetch_one(&mut **conn)
    .await?;
    Ok(released.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("valid instant")
            .with_timezone(&Utc)
    }

    #[test]
    fn classifies_whole_old_chunks_as_full_drops() {
        let now = instant("2026-11-19T12:00:00Z");
        let cutoff = now - ChronoDuration::days(2);
        let drop_before = chunk_retention_cutoff(now, 2);

        assert_eq!(
            raw_chunk_branch(
                instant("2026-11-16T00:00:00Z"),
                instant("2026-11-17T00:00:00Z"),
                cutoff,
                drop_before,
            ),
            Some(RawChunkBranch::FullDrop)
        );
    }

    #[test]
    fn classifies_a_chunk_straddling_the_exact_cutoff_as_boundary() {
        let now = instant("2026-11-19T12:00:00Z");
        let cutoff = now - ChronoDuration::days(2);
        let drop_before = chunk_retention_cutoff(now, 2);

        assert_eq!(
            raw_chunk_branch(
                instant("2026-11-17T00:00:00Z"),
                instant("2026-11-18T00:00:00Z"),
                cutoff,
                drop_before,
            ),
            Some(RawChunkBranch::Boundary)
        );
        assert_eq!(
            raw_chunk_branch(
                instant("2026-11-18T00:00:00Z"),
                instant("2026-11-19T00:00:00Z"),
                cutoff,
                drop_before,
            ),
            None
        );
    }

    #[test]
    fn never_classifies_recent_or_current_day_chunks() {
        let now = instant("2026-11-19T12:00:00Z");
        let cutoff = now - ChronoDuration::days(2);
        let drop_before = chunk_retention_cutoff(now, 2);

        assert_eq!(
            raw_chunk_branch(
                instant("2026-11-19T00:00:00Z"),
                instant("2026-11-20T00:00:00Z"),
                cutoff,
                drop_before,
            ),
            None
        );
        assert_eq!(
            raw_chunk_branch(
                instant("2026-11-19T13:00:00Z"),
                instant("2026-11-20T00:00:00Z"),
                cutoff,
                drop_before,
            ),
            None
        );
    }

    #[test]
    fn accepts_only_one_dropped_chunk_with_the_listed_identity() {
        assert!(
            verify_dropped_chunk(
                &["_hyper_1_7_chunk".into()],
                "_timescaledb_internal",
                "_hyper_1_7_chunk"
            )
            .is_ok()
        );
        assert!(
            verify_dropped_chunk(
                &["_timescaledb_internal._hyper_1_7_chunk".into()],
                "_timescaledb_internal",
                "_hyper_1_7_chunk"
            )
            .is_ok()
        );

        assert!(verify_dropped_chunk(&[], "_timescaledb_internal", "_hyper_1_7_chunk").is_err());
        assert!(
            verify_dropped_chunk(
                &["_hyper_1_7_chunk".into(), "_hyper_1_8_chunk".into()],
                "_timescaledb_internal",
                "_hyper_1_7_chunk"
            )
            .is_err()
        );
        assert!(
            verify_dropped_chunk(
                &["_hyper_1_8_chunk".into()],
                "_timescaledb_internal",
                "_hyper_1_7_chunk"
            )
            .is_err()
        );
    }
}
