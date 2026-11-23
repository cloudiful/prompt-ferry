use super::*;
use crate::storage_sanitization::{
    SanitizationStats, sanitize_json_for_storage, sanitize_text_for_storage,
};
use crate::usage_prompt::PromptBlockSeed;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

fn prepare_prompt_blocks(
    items: &[PromptBlockSeed],
    refs: &[PromptMessageRef],
) -> (Vec<Value>, SanitizationStats) {
    assert_eq!(items.len(), refs.len());
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut stats = SanitizationStats::default();
    for (item, reference) in items.iter().zip(refs) {
        let (content_json, json_stats) = sanitize_json_for_storage(&item.content_json);
        let (preview_text, text_stats) = sanitize_text_for_storage(&item.preview_text);
        stats.nul_count = stats
            .nul_count
            .saturating_add(json_stats.nul_count)
            .saturating_add(text_stats.nul_count);
        if seen.insert(&reference.block_hash) {
            rows.push(serde_json::json!({
                "block_hash": reference.block_hash,
                "role": item.role,
                "content_json": { "value": content_json },
                "preview_text": preview_text,
            }));
        }
    }
    (rows, stats)
}

pub async fn upsert_usage_prompt_block(
    pool: &PgPool,
    created_at: DateTime<Utc>,
    items: &[PromptBlockSeed],
    refs: &[PromptMessageRef],
) -> Result<SanitizationStats> {
    let (rows, stats) = prepare_prompt_blocks(items, refs);
    if rows.is_empty() {
        return Ok(stats);
    }
    let created_at = prompt_block_day(created_at);
    let rows = Value::Array(rows);
    sqlx::query_file!(
        "src/sql/usage/upsert_usage_prompt_blocks.sql",
        created_at,
        rows,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage_prompt::{prompt_block_hash, prompt_message_refs};
    use chrono::TimeZone;

    #[test]
    fn batch_preserves_first_payload_and_counts_every_occurrence() {
        let (empty, stats) = prepare_prompt_blocks(&[], &[]);
        assert!(empty.is_empty());
        assert_eq!(stats.nul_count, 0);
        let first = PromptBlockSeed {
            role: "user".into(),
            content_json: serde_json::json!({"text": "a\0b"}),
            preview_text: "first\0".into(),
        };
        let second = PromptBlockSeed {
            preview_text: "second\0\0".into(),
            ..first.clone()
        };
        let items = vec![first, second];
        let refs = prompt_message_refs(&items);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0], refs[1]);
        assert_eq!(
            refs[0].block_hash,
            prompt_block_hash(&items[0].role, &items[0].content_json)
        );
        let (rows, stats) = prepare_prompt_blocks(&items, &refs);
        assert_eq!(stats.nul_count, 5);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["block_hash"], refs[0].block_hash);
        assert_eq!(
            rows[0]["content_json"]["value"],
            serde_json::json!({"text": "ab"})
        );
        let null_item = PromptBlockSeed {
            role: "user".into(),
            content_json: Value::Null,
            preview_text: String::new(),
        };
        let null_refs = prompt_message_refs(std::slice::from_ref(&null_item));
        let (null_rows, _) = prepare_prompt_blocks(&[null_item], &null_refs);
        assert_eq!(
            null_rows[0]["content_json"],
            serde_json::json!({"value": null})
        );
        assert_eq!(rows[0]["preview_text"], "first");
    }

    #[test]
    fn prompt_block_day_uses_utc_midnight() {
        let before = Utc.with_ymd_and_hms(2026, 11, 23, 23, 59, 59).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 11, 24, 0, 0, 0).unwrap();
        assert_eq!(
            prompt_block_day(before),
            Utc.with_ymd_and_hms(2026, 11, 23, 0, 0, 0).unwrap()
        );
        assert_eq!(prompt_block_day(after), after);
        assert_ne!(prompt_block_day(before), prompt_block_day(after));
    }
}
