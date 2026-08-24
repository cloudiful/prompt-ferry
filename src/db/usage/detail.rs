use super::*;
use crate::db::{
    RequestAbortReason, RequestFailureFamily, RequestRecordCategory, RequestRecordRedactionSummary,
    RequestRecordState, RouteSelectionReason,
};
use crate::raw_payload_store::RawPayloadStore;
use tracing::warn;

#[derive(sqlx::FromRow)]
struct RequestRecordDetailRow {
    record_id: i64,
    request_id: uuid::Uuid,
    request_category: RequestRecordCategory,
    user_id: Option<i64>,
    user_login_name: Option<String>,
    client_key_label: Option<String>,
    request_user_agent: Option<String>,
    http_request_content_encoding: Option<String>,
    http_request_compressed: bool,
    http_request_compressed_bytes: Option<i64>,
    http_request_decompressed_bytes: Option<i64>,
    http_request_compression_ratio: Option<f64>,
    endpoint_id: Option<uuid::Uuid>,
    endpoint_name: Option<String>,
    endpoint_key_id: Option<uuid::Uuid>,
    endpoint_key_label: Option<String>,
    mcp_server_id: Option<uuid::Uuid>,
    mcp_server_name: Option<String>,
    mcp_protocol_method: Option<String>,
    mcp_operation_name: Option<String>,
    path: String,
    model: Option<String>,
    request_state: RequestRecordState,
    status: Option<i32>,
    ok: Option<bool>,
    duration_ms: Option<i64>,
    ttft_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    cache_rate: Option<f64>,
    conversation_id: Option<uuid::Uuid>,
    parent_event_id: Option<i64>,
    conversation_seq: Option<i32>,
    conversation_source: String,
    storage_sanitized: bool,
    storage_sanitized_nul_count: i32,
    applied: bool,
    findings_count: i32,
    replacements_count: i32,
    types: Vec<String>,
    fields: Vec<String>,
    client_installation_id: Option<String>,
    normalized_item_count: Option<i32>,
    request_storage_mode: String,
    raw_object_key: Option<String>,
    raw_object_sha256: Option<String>,
    raw_object_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    request_has_previous_response_id: bool,
    request_previous_response_id: Option<String>,
    request_previous_response_parent_found: Option<bool>,
    request_conversation_key: Option<String>,
    request_conversation_parent_found: Option<bool>,
    provider_response_id: Option<String>,
    has_full_request: bool,
    has_parent: bool,
    response_prompt: Option<String>,
    assistant_message_json: Option<serde_json::Value>,
    assistant_output_items_json: Option<serde_json::Value>,
    has_reasoning_content: Option<bool>,
    upstream_error_body: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    abort_reason: Option<RequestAbortReason>,
    abort_from_state: Option<RequestRecordState>,
    abort_response_started: Option<bool>,
    failure_family: Option<RequestFailureFamily>,
    mcp_bearer_token_slot: Option<i16>,
    route_selection_reason: RouteSelectionReason,
    response_capture_truncated: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<RequestRecordDetailRow> for RequestRecordDetail {
    fn from(row: RequestRecordDetailRow) -> Self {
        Self {
            record_id: row.record_id,
            request_id: row.request_id,
            request_category: row.request_category,
            user_id: row.user_id,
            user_login_name: row.user_login_name,
            client_key_label: row.client_key_label,
            request_user_agent: row.request_user_agent,
            http_request_content_encoding: row.http_request_content_encoding,
            http_request_compressed: row.http_request_compressed,
            http_request_compressed_bytes: row.http_request_compressed_bytes,
            http_request_decompressed_bytes: row.http_request_decompressed_bytes,
            http_request_compression_ratio: row.http_request_compression_ratio,
            endpoint_id: row.endpoint_id,
            endpoint_name: row.endpoint_name,
            endpoint_key_id: row.endpoint_key_id,
            endpoint_key_label: row.endpoint_key_label,
            mcp_server_id: row.mcp_server_id,
            mcp_server_name: row.mcp_server_name,
            mcp_protocol_method: row.mcp_protocol_method,
            mcp_operation_name: row.mcp_operation_name,
            path: row.path,
            model: row.model,
            request_state: row.request_state,
            status: row.status,
            ok: row.ok,
            duration_ms: row.duration_ms,
            ttft_ms: row.ttft_ms,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            total_tokens: row.total_tokens,
            cached_tokens: row.cached_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_write_tokens: row.cache_write_tokens,
            cache_rate: row.cache_rate,
            conversation_id: row.conversation_id,
            parent_event_id: row.parent_event_id,
            conversation_seq: row.conversation_seq,
            conversation_source: row.conversation_source,
            storage_sanitized: row.storage_sanitized,
            storage_sanitized_nul_count: row.storage_sanitized_nul_count,
            redaction: RequestRecordRedactionSummary {
                applied: row.applied,
                findings_count: row.findings_count,
                replacements_count: row.replacements_count,
                types: row.types,
                fields: row.fields,
            },
            client_installation_id: row.client_installation_id,
            normalized_item_count: row.normalized_item_count,
            request_storage_mode: row.request_storage_mode,
            // Raw bodies never live in PostgreSQL; they are overlaid from the
            // selected object store below when available.
            request_raw_json: None,
            request_has_previous_response_id: row.request_has_previous_response_id,
            request_previous_response_id: row.request_previous_response_id,
            request_previous_response_parent_found: row.request_previous_response_parent_found,
            request_conversation_key: row.request_conversation_key,
            request_conversation_parent_found: row.request_conversation_parent_found,
            provider_response_id: row.provider_response_id,
            has_full_request: row.has_full_request,
            has_parent: row.has_parent,
            response_prompt: row.response_prompt,
            response_raw_body: None,
            assistant_message_json: row.assistant_message_json,
            assistant_output_items_json: row.assistant_output_items_json,
            has_reasoning_content: row.has_reasoning_content,
            upstream_error_body: row.upstream_error_body,
            error_code: row.error_code,
            error_message: row.error_message,
            abort_reason: row.abort_reason,
            abort_from_state: row.abort_from_state,
            abort_response_started: row.abort_response_started,
            failure_family: row.failure_family,
            mcp_bearer_token_slot: row.mcp_bearer_token_slot,
            route_selection_reason: row.route_selection_reason,
            response_capture_truncated: row.response_capture_truncated,
            tool_call_events: Vec::new(),
            created_at: row.created_at,
        }
    }
}

pub async fn get_visible_usage_event_detail(
    pool: &PgPool,
    event_id: i64,
    visible_user_id: Option<i64>,
) -> Result<Option<RequestRecordDetail>> {
    Ok(sqlx::query_file_as!(
        RequestRecordDetailRow,
        "src/sql/usage/get_visible_usage_event_detail.sql",
        event_id,
        visible_user_id,
    )
    .fetch_optional(pool)
    .await?
    .map(Into::into))
}

pub(crate) async fn get_visible_usage_event_detail_with_raw_store(
    pool: &PgPool,
    event_id: i64,
    visible_user_id: Option<i64>,
    raw_store: Option<&RawPayloadStore>,
) -> Result<Option<RequestRecordDetail>> {
    let Some(row) = sqlx::query_file_as!(
        RequestRecordDetailRow,
        "src/sql/usage/get_visible_usage_event_detail.sql",
        event_id,
        visible_user_id,
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let object_key = row.raw_object_key.clone();
    let object_sha256 = row.raw_object_sha256.clone();
    let object_expires_at = row.raw_object_expires_at;
    let mut detail = RequestRecordDetail::from(row);
    let Some(raw_store) = raw_store else {
        return Ok(Some(detail));
    };
    if object_key.is_none() || object_expires_at.is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return Ok(Some(detail));
    }
    match raw_store
        .get(
            object_key.as_deref().unwrap_or_default(),
            object_sha256.as_deref(),
        )
        .await
    {
        Ok(Some(payload)) => {
            if payload.request_raw_json.is_some() {
                detail.request_raw_json = payload.request_raw_json;
            }
            if payload.response_raw_body.is_some() {
                detail.response_raw_body = payload.response_raw_body;
            }
        }
        Ok(None) => {}
        Err(error) => {
            warn!(error = %error, event_id, "failed to load raw payload object for usage detail")
        }
    }
    Ok(Some(detail))
}

pub async fn get_usage_event_chain_entry(
    pool: &PgPool,
    event_id: i64,
) -> Result<Option<RequestRecordChainEntry>> {
    Ok(sqlx::query_file_as!(
        RequestRecordChainEntry,
        "src/sql/usage/get_usage_event_chain_entry.sql",
        event_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_visible_usage_event_chain_entry(
    pool: &PgPool,
    event_id: i64,
    visible_user_id: Option<i64>,
) -> Result<Option<RequestRecordChainEntry>> {
    Ok(sqlx::query_file_as!(
        RequestRecordChainEntry,
        "src/sql/usage/get_visible_usage_event_chain_entry.sql",
        event_id,
        visible_user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_usage_event_by_request_id(
    pool: &PgPool,
    user_id: Option<i64>,
    request_id: uuid::Uuid,
) -> Result<Option<RequestRecordChainEntry>> {
    let Some(locator) = get_usage_event_locator_by_request_id(pool, user_id, request_id).await?
    else {
        return Ok(None);
    };
    get_usage_event_chain_entry(pool, locator.event_id).await
}

pub async fn get_usage_event_by_provider_response_id(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_response_id: &str,
) -> Result<Option<RequestRecordChainEntry>> {
    let Some(locator) =
        get_usage_event_locator_by_provider_response_id(pool, user_id, provider_response_id)
            .await?
    else {
        return Ok(None);
    };
    get_usage_event_chain_entry(pool, locator.event_id).await
}

pub async fn get_usage_event_by_provider_conversation_key(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_conversation_key: &str,
) -> Result<Option<RequestRecordChainEntry>> {
    let Some(locator) = get_usage_event_locator_by_provider_conversation_key(
        pool,
        user_id,
        provider_conversation_key,
    )
    .await?
    else {
        return Ok(None);
    };
    get_usage_event_chain_entry(pool, locator.event_id).await
}

pub async fn get_replayable_usage_event_by_provider_conversation_key(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_conversation_key: &str,
) -> Result<Option<RequestRecordChainEntry>> {
    let Some(locator) = get_replayable_usage_event_locator_by_provider_conversation_key(
        pool,
        user_id,
        provider_conversation_key,
    )
    .await?
    else {
        return Ok(None);
    };
    get_usage_event_chain_entry(pool, locator.event_id).await
}

pub async fn latest_usage_event_by_conversation(
    pool: &PgPool,
    user_id: Option<i64>,
    conversation_id: uuid::Uuid,
) -> Result<Option<RequestRecordChainEntry>> {
    let Some(locator) =
        latest_usage_event_locator_by_conversation(pool, user_id, conversation_id).await?
    else {
        return Ok(None);
    };
    get_usage_event_chain_entry(pool, locator.event_id).await
}

pub async fn get_usage_event_locator_by_request_id(
    pool: &PgPool,
    user_id: Option<i64>,
    request_id: uuid::Uuid,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/get_usage_event_by_request_id.sql",
        request_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_usage_event_locator_by_provider_response_id(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_response_id: &str,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/get_usage_event_by_provider_response_id.sql",
        provider_response_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_usage_event_locator_by_provider_conversation_key(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_conversation_key: &str,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/get_usage_event_by_provider_conversation_key.sql",
        provider_conversation_key,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn get_replayable_usage_event_locator_by_provider_conversation_key(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_conversation_key: &str,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/get_replayable_usage_event_by_provider_conversation_key.sql",
        provider_conversation_key,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn latest_usage_event_locator_by_conversation(
    pool: &PgPool,
    user_id: Option<i64>,
    conversation_id: uuid::Uuid,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/latest_usage_event_by_conversation.sql",
        conversation_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn latest_replayable_usage_event_locator_by_conversation(
    pool: &PgPool,
    user_id: Option<i64>,
    conversation_id: uuid::Uuid,
) -> Result<Option<RequestRecordConversationLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordConversationLocator,
        "src/sql/usage/latest_replayable_usage_event_by_conversation.sql",
        conversation_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn latest_usage_event_locator_by_provider_conversation_key(
    pool: &PgPool,
    user_id: Option<i64>,
    provider_conversation_key: &str,
) -> Result<Option<RequestRecordConversationLocator>> {
    get_usage_event_locator_by_provider_conversation_key(pool, user_id, provider_conversation_key)
        .await
}
