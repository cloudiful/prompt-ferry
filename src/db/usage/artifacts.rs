use super::*;
use crate::storage_sanitization::{SanitizationStats, sanitize_json_for_storage};

pub async fn upsert_usage_assistant_artifact(
    pool: &PgPool,
    input: RequestRecordAssistantArtifactCreate,
) -> Result<SanitizationStats> {
    let (message_json, stats) = sanitize_json_for_storage(&input.message_json);
    sqlx::query_file!(
        "src/sql/usage/upsert_usage_assistant_artifact.sql",
        input.event_id,
        message_json,
        input.has_reasoning_content,
        input.has_tool_calls,
    )
    .execute(pool)
    .await?;
    Ok(stats)
}

pub async fn get_usage_assistant_artifacts(
    pool: &PgPool,
    event_ids: &[i64],
) -> Result<Vec<RequestRecordAssistantArtifact>> {
    if event_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_file_as!(
        RequestRecordAssistantArtifact,
        "src/sql/usage/get_usage_assistant_artifacts.sql",
        event_ids,
    )
    .fetch_all(pool)
    .await?)
}
