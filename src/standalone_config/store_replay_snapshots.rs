use sqlx::Row;
use uuid::Uuid;

use crate::standalone_config::{
    ReplaySnapshotUpsertOutcome, Result, StandaloneConfigError, StandaloneReplaySnapshotRecord,
};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    // ---- Durable standalone replay snapshot store (Phase 1C-b). ----

    /// Monotonically upsert the latest replay snapshot for a single
    /// conversation. The SQL `ON CONFLICT ... DO UPDATE ... WHERE`
    /// gate ensures the existing row is preserved when the incoming
    /// snapshot would regress the stored checkpoint by `(conversation_seq,
    /// base_event_id)` ordering. Raw body payloads remain out of scope:
    /// `prompt_refs_json` carries only role and block-hash references,
    /// validated here as a non-empty JSON array.
    pub async fn upsert_replay_snapshot(
        &self,
        snapshot: &StandaloneReplaySnapshotRecord,
    ) -> Result<ReplaySnapshotUpsertOutcome> {
        if snapshot.conversation_seq <= 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "conversation_seq",
                message: "must be a positive integer".to_string(),
            });
        }
        if snapshot.base_event_id < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "base_event_id",
                message: "must be non-negative".to_string(),
            });
        }
        if snapshot.ref_count < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "ref_count",
                message: "must be non-negative".to_string(),
            });
        }
        if snapshot.byte_size < 0 {
            return Err(StandaloneConfigError::InvalidInput {
                field: "byte_size",
                message: "must be non-negative".to_string(),
            });
        }
        validate_prompt_refs_json(&snapshot.prompt_refs_json)?;
        let updated_at = snapshot.updated_at.to_rfc3339();
        let existed = standalone_query!("src/sql/standalone/get_replay_snapshot.sql")
            .bind(snapshot.conversation_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let result = standalone_query!("src/sql/standalone/upsert_replay_snapshot.sql")
            .bind(snapshot.conversation_id.to_string())
            .bind(snapshot.base_event_id)
            .bind(snapshot.conversation_seq)
            .bind(&snapshot.prompt_refs_json)
            .bind(snapshot.ref_count)
            .bind(snapshot.byte_size)
            .bind(&updated_at)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Ok(ReplaySnapshotUpsertOutcome::Skipped);
        }
        Ok(if existed.is_some() {
            ReplaySnapshotUpsertOutcome::Updated
        } else {
            ReplaySnapshotUpsertOutcome::Inserted
        })
    }

    /// Fetch the latest replay snapshot for a conversation, or `None`
    /// when the conversation has no persisted checkpoint. A malformed
    /// row (bad UUID, unparseable timestamp, invalid prompt-refs JSON)
    /// is reported as `StandaloneConfigError::CorruptDatabase` so the
    /// hydration path can decide whether to skip or fail the entire
    /// load.
    pub async fn get_replay_snapshot(
        &self,
        conversation_id: Uuid,
    ) -> Result<Option<StandaloneReplaySnapshotRecord>> {
        let row = standalone_query!("src/sql/standalone/get_replay_snapshot.sql")
            .bind(conversation_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        parse_replay_snapshot_row(&row)
    }
}

/// Validate that a `prompt_refs_json` payload is a non-empty JSON array
/// of objects carrying `role` (string) and `block_hash` (string)
/// entries. This is the same shape used by
/// `db::decode_prompt_message_refs` on PostgreSQL, but applied here as
/// a storage-layer guard so a corrupt caller cannot poison the durable
/// snapshot.
fn validate_prompt_refs_json(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must not be empty".to_string(),
        });
    }
    let parsed: serde_json::Value =
        serde_json::from_str(value).map_err(|error| StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: format!("not valid JSON: {error}"),
        })?;
    let entries = parsed
        .as_array()
        .ok_or_else(|| StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must be a JSON array".to_string(),
        })?;
    if entries.is_empty() {
        return Err(StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            message: "must contain at least one entry".to_string(),
        });
    }
    for (index, entry) in entries.iter().enumerate() {
        let object = entry
            .as_object()
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} is not an object"),
            })?;
        let role = object
            .get("role")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} role is not a string"),
            })?;
        if role.is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} role is empty"),
            });
        }
        let block_hash = object
            .get("block_hash")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} block_hash is not a string"),
            })?;
        if block_hash.is_empty() {
            return Err(StandaloneConfigError::InvalidInput {
                field: "prompt_refs_json",
                message: format!("entry {index} block_hash is empty"),
            });
        }
    }
    Ok(())
}

/// Map a `standalone_replay_snapshots` row into the storage DTO. Errors
/// from column-level decoding propagate as
/// `StandaloneConfigError::CorruptDatabase` so the caller can decide
/// whether to drop the row or fail the hydration.
fn parse_replay_snapshot_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<StandaloneReplaySnapshotRecord>> {
    let conversation_id: String = row.try_get("conversation_id")?;
    let conversation_id = Uuid::parse_str(&conversation_id).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!(
            "column conversation_id is not a UUID: {error}"
        ))
    })?;
    let base_event_id: i64 = row.try_get("base_event_id")?;
    if base_event_id < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column base_event_id is negative: {base_event_id}"
        )));
    }
    let conversation_seq: i64 = row.try_get("conversation_seq")?;
    if conversation_seq <= 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column conversation_seq is not positive: {conversation_seq}"
        )));
    }
    let prompt_refs_json: String = row.try_get("prompt_refs_json")?;
    let ref_count: i64 = row.try_get("ref_count")?;
    if ref_count < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column ref_count is negative: {ref_count}"
        )));
    }
    let byte_size: i64 = row.try_get("byte_size")?;
    if byte_size < 0 {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column byte_size is negative: {byte_size}"
        )));
    }
    let updated_at: String = row.try_get("updated_at")?;
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at)
        .map(|value| value.with_timezone(&chrono::Utc))
        .map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column updated_at is not RFC3339: {error}"
            ))
        })?;
    validate_prompt_refs_json(&prompt_refs_json).map_err(|error| match error {
        StandaloneConfigError::InvalidInput { field, message } => {
            StandaloneConfigError::CorruptDatabase(format!("{field} {message}"))
        }
        other => other,
    })?;
    Ok(Some(StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id,
        conversation_seq: i32::try_from(conversation_seq).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column conversation_seq is outside the i32 range".to_string(),
            )
        })?,
        prompt_refs_json,
        ref_count: i32::try_from(ref_count).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column ref_count is outside the i32 range".to_string(),
            )
        })?,
        byte_size: i32::try_from(byte_size).map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "column byte_size is outside the i32 range".to_string(),
            )
        })?,
        updated_at,
    }))
}
