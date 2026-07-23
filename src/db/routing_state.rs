use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::ConversationEndpointOverride;

pub async fn get_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<Option<ConversationEndpointOverride>> {
    Ok(sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/get_conversation_endpoint_override.sql",
        conversation_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn upsert_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key_id: Option<Uuid>,
    created_by_user_id: i64,
) -> Result<ConversationEndpointOverride> {
    Ok(sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/upsert_conversation_endpoint_override.sql",
        conversation_id,
        endpoint_id,
        endpoint_key_id,
        created_by_user_id,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn clear_conversation_endpoint_key_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/routes/clear_conversation_endpoint_key_override.sql",
        conversation_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/routes/delete_conversation_endpoint_override.sql",
        conversation_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
