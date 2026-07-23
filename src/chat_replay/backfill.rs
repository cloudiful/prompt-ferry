use anyhow::Result;
use sqlx::PgPool;

use crate::db;

use super::assembly::fallback_artifact_for_entry;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplayBackfillStats {
    pub scanned: usize,
    pub repaired: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub async fn backfill_missing_assistant_artifacts(
    pool: &PgPool,
    apply: bool,
) -> Result<ReplayBackfillStats> {
    let entries = db::list_usage_events_missing_assistant_artifacts(pool).await?;
    let mut stats = ReplayBackfillStats::default();
    for entry in entries {
        stats.scanned += 1;
        let Some(artifact) = fallback_artifact_for_entry(&entry) else {
            stats.skipped += 1;
            continue;
        };
        if !apply {
            stats.repaired += 1;
            continue;
        }
        match db::upsert_usage_assistant_artifact(
            pool,
            db::UsageAssistantArtifactCreate {
                event_id: entry.event_id,
                message_json: artifact.message_json,
                has_reasoning_content: artifact.has_reasoning_content,
                has_tool_calls: artifact.has_tool_calls,
            },
        )
        .await
        {
            Ok(_) => stats.repaired += 1,
            Err(_) => stats.failed += 1,
        }
    }
    Ok(stats)
}
