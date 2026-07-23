use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{ConversationRedactionSessionCreate, ConversationRedactionSessionRow};

pub async fn get_conversation_redaction_session(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<Option<ConversationRedactionSessionRow>> {
    sqlx::query_file_as!(
        ConversationRedactionSessionRow,
        "src/sql/usage/get_conversation_redaction_session.sql",
        conversation_id
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub async fn upsert_conversation_redaction_session(
    pool: &PgPool,
    input: ConversationRedactionSessionCreate,
) -> Result<()> {
    sqlx::query_file!(
        "src/sql/usage/upsert_conversation_redaction_session.sql",
        input.conversation_id,
        input.session_ciphertext,
        input.session_nonce,
        input.session_key_version,
        input.last_event_id,
    )
    .execute(pool)
    .await?;
    Ok(())
}
