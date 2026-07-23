use super::*;

pub async fn insert_replay_snapshot(pool: &PgPool, input: ReplaySnapshotCreate) -> Result<()> {
    sqlx::query_file!(
        "src/sql/usage/insert_replay_snapshot.sql",
        input.event_id,
        input.conversation_id,
        input.conversation_seq,
        input.base_event_id,
        input.prompt_refs_json,
        input.ref_count,
        input.byte_size,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn latest_replay_snapshot(
    pool: &PgPool,
    conversation_id: uuid::Uuid,
) -> Result<Option<ReplaySnapshotRow>> {
    Ok(sqlx::query_file_as!(
        ReplaySnapshotRow,
        "src/sql/usage/latest_replay_snapshot.sql",
        conversation_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn replay_snapshot_before_or_at_seq(
    pool: &PgPool,
    conversation_id: uuid::Uuid,
    conversation_seq: i32,
) -> Result<Option<ReplaySnapshotRow>> {
    Ok(sqlx::query_file_as!(
        ReplaySnapshotRow,
        "src/sql/usage/replay_snapshot_before_or_at_seq.sql",
        conversation_id,
        conversation_seq,
    )
    .fetch_optional(pool)
    .await?)
}
