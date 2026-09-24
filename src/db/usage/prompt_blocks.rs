use super::*;
use crate::storage_sanitization::{
    SanitizationStats, sanitize_json_for_storage, sanitize_text_for_storage,
};
use chrono::{DateTime, Utc};

pub async fn upsert_usage_prompt_block(
    pool: &PgPool,
    created_at: DateTime<Utc>,
    block_hash: &str,
    role: &str,
    content_json: &Value,
    preview_text: &str,
) -> Result<SanitizationStats> {
    let (content_json, json_stats) = sanitize_json_for_storage(content_json);
    let (preview_text, text_stats) = sanitize_text_for_storage(preview_text);
    let mut stats = json_stats;
    stats.merge(text_stats);
    // Issue #277 Phase P7: blocks are partitioned by day, so the dedupe key is
    // `(block_hash, created_at day)`; UTC midnight keeps one row per block per
    // day instead of one row per request.
    let created_at = prompt_block_day(created_at);
    sqlx::query_file!(
        "src/sql/usage/upsert_usage_prompt_block.sql",
        created_at,
        block_hash,
        role,
        content_json,
        preview_text,
    )
    .execute(pool)
    .await?;
    Ok(stats)
}

/// UTC midnight of the instant's day.
fn prompt_block_day(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|value| value.and_utc())
        .unwrap_or(created_at)
}

pub async fn get_usage_prompt_blocks(
    pool: &PgPool,
    hashes: &[String],
) -> Result<Vec<RequestRecordPromptBlock>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_file_as!(
        RequestRecordPromptBlock,
        "src/sql/usage/get_usage_prompt_blocks.sql",
        hashes,
    )
    .fetch_all(pool)
    .await?)
}

pub fn decode_prompt_message_refs(value: &Value) -> Result<Vec<PromptMessageRef>> {
    Ok(serde_json::from_value::<Vec<PromptMessageRef>>(
        value.clone(),
    )?)
}
