use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{ConversationRedactionSessionCreate, ConversationRedactionSessionRow};

pub async fn get_conversation_redaction_session(
    pool: &PgPool,
    conversation_id: Uuid,
    policy_version: i64,
) -> Result<Option<ConversationRedactionSessionRow>> {
    sqlx::query_file_as!(
        ConversationRedactionSessionRow,
        "src/sql/usage/get_conversation_redaction_session.sql",
        conversation_id,
        policy_version
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

/// Returns the affected row count. `0` means the conditional upsert kept a row
/// that was already newer (higher `last_event_id` for the same policy version),
/// so the caller can warn about a dropped mapping.
pub async fn upsert_conversation_redaction_session(
    pool: &PgPool,
    input: ConversationRedactionSessionCreate,
) -> Result<u64> {
    let result = sqlx::query_file!(
        "src/sql/usage/upsert_conversation_redaction_session.sql",
        input.conversation_id,
        input.session_ciphertext,
        input.session_nonce,
        input.session_key_version,
        input.last_event_id,
        input.policy_version,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete_conversation_redaction_session(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<u64> {
    let result = sqlx::query_file!(
        "src/sql/usage/delete_conversation_redaction_session.sql",
        conversation_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
