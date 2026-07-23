use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::db::types::{
    PromptMessageRef, ReplaySnapshotCreate, ReplaySnapshotRow, RequestRecordAssistantArtifact,
    RequestRecordAssistantArtifactCreate, RequestRecordChainEntry, RequestRecordClearQuery,
    RequestRecordConversationLocator, RequestRecordCreate, RequestRecordDetail,
    RequestRecordFacets, RequestRecordListRow, RequestRecordPage, RequestRecordPromptBlock,
    RequestRecordQuery, RequestRecordState, RequestRecordSummary, UsageClearScope,
};

mod artifacts;
mod buckets;
mod cleanup;
mod detail;
mod insert;
mod overview;
mod prompt_blocks;
mod quality;
mod query;
mod raw_partitions;
mod redaction_sessions;
mod replay_snapshots;
mod runtime;

pub use artifacts::list_usage_events_missing_assistant_artifacts;
pub use artifacts::{get_usage_assistant_artifacts, upsert_usage_assistant_artifact};
pub use buckets::usage_buckets;
pub use cleanup::{
    RawPayloadMaintenanceReport, clear_usage_events, prune_usage_events,
    run_raw_payload_maintenance,
};
pub use detail::{
    get_replayable_usage_event_by_provider_conversation_key,
    get_replayable_usage_event_locator_by_provider_conversation_key,
    get_usage_event_by_provider_conversation_key, get_usage_event_by_provider_response_id,
    get_usage_event_by_request_id, get_usage_event_chain_entry,
    get_usage_event_locator_by_provider_conversation_key,
    get_usage_event_locator_by_provider_response_id, get_usage_event_locator_by_request_id,
    get_visible_usage_event_chain_entry, get_visible_usage_event_detail,
    latest_replayable_usage_event_locator_by_conversation, latest_usage_event_by_conversation,
    latest_usage_event_locator_by_conversation,
    latest_usage_event_locator_by_provider_conversation_key,
};
pub use insert::{record_request_record, record_request_state};
pub use overview::{OverviewBucket, OverviewWindow, request_records_overview};
pub use prompt_blocks::{
    decode_prompt_message_refs, get_usage_prompt_blocks, upsert_usage_prompt_block,
};
pub use query::{list_request_record_facets, list_request_records, request_record_summary};
pub use redaction_sessions::{
    get_conversation_redaction_session, upsert_conversation_redaction_session,
};
pub use replay_snapshots::{
    insert_replay_snapshot, latest_replay_snapshot, replay_snapshot_before_or_at_seq,
};
pub use runtime::{
    abort_request_records_by_ids, abort_stale_request_records, allocate_conversation_seq,
    delete_request_record_lease, find_request_record_tool_calls_by_call_ids,
    heartbeat_request_record_lease, list_active_request_record_ids, list_request_record_tool_calls,
    upsert_request_record_tool_call,
};
