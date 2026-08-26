use super::*;
use crate::db::{
    ClientKeyFacet, RequestAbortReason, RequestFailureFamily, RequestRecordCategory,
    RequestRecordRedactionSummary, RequestRecordState, RouteSelectionReason,
};

mod sort;

use sort::request_records_order_by_clause;

const REQUEST_RECORDS_PAGE_SQL: &str = include_str!("../../../sql/usage_events_page.sql");

#[derive(sqlx::FromRow)]
struct RequestRecordListRowFlat {
    record_id: i64,
    request_id: uuid::Uuid,
    request_category: Option<RequestRecordCategory>,
    user_id: Option<i64>,
    user_login_name: Option<String>,
    client_key_label: Option<String>,
    endpoint_id: Option<uuid::Uuid>,
    endpoint_name: Option<String>,
    mcp_server_id: Option<uuid::Uuid>,
    mcp_server_name: Option<String>,
    mcp_protocol_method: Option<String>,
    mcp_operation_name: Option<String>,
    path: Option<String>,
    model: Option<String>,
    request_state: Option<RequestRecordState>,
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
    conversation_source: Option<String>,
    storage_sanitized: Option<bool>,
    storage_sanitized_nul_count: Option<i32>,
    applied: Option<bool>,
    findings_count: Option<i32>,
    replacements_count: Option<i32>,
    types: Option<Vec<String>>,
    fields: Option<Vec<String>>,
    has_full_request: Option<bool>,
    has_parent: Option<bool>,
    error_code: Option<String>,
    error_message: Option<String>,
    abort_reason: Option<RequestAbortReason>,
    abort_from_state: Option<RequestRecordState>,
    abort_response_started: Option<bool>,
    failure_family: Option<RequestFailureFamily>,
    mcp_bearer_token_slot: Option<i16>,
    route_selection_reason: Option<RouteSelectionReason>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<RequestRecordListRowFlat> for RequestRecordListRow {
    fn from(row: RequestRecordListRowFlat) -> Self {
        Self {
            record_id: row.record_id,
            request_id: row.request_id,
            request_category: row.request_category.unwrap_or_default(),
            user_id: row.user_id,
            user_login_name: row.user_login_name,
            client_key_label: row.client_key_label,
            endpoint_id: row.endpoint_id,
            endpoint_name: row.endpoint_name,
            mcp_server_id: row.mcp_server_id,
            mcp_server_name: row.mcp_server_name,
            mcp_protocol_method: row.mcp_protocol_method,
            mcp_operation_name: row.mcp_operation_name,
            path: row.path.unwrap_or_default(),
            model: row.model,
            request_state: row.request_state.unwrap_or(RequestRecordState::Aborted),
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
            conversation_source: row.conversation_source.unwrap_or_else(|| "none".to_string()),
            storage_sanitized: row.storage_sanitized.unwrap_or(false),
            storage_sanitized_nul_count: row.storage_sanitized_nul_count.unwrap_or(0),
            redaction: RequestRecordRedactionSummary {
                applied: row.applied.unwrap_or(false),
                findings_count: row.findings_count.unwrap_or(0),
                replacements_count: row.replacements_count.unwrap_or(0),
                types: row.types.unwrap_or_default(),
                fields: row.fields.unwrap_or_default(),
            },
            has_full_request: row.has_full_request.unwrap_or(false),
            has_parent: row.has_parent.unwrap_or(false),
            error_code: row.error_code,
            error_message: row.error_message,
            abort_reason: row.abort_reason,
            abort_from_state: row.abort_from_state,
            abort_response_started: row.abort_response_started,
            failure_family: row.failure_family,
            mcp_bearer_token_slot: row.mcp_bearer_token_slot,
            route_selection_reason: row.route_selection_reason.unwrap_or_default(),
            created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
        }
    }
}

pub async fn request_record_summary(
    pool: &PgPool,
    days: i64,
    visible_user_id: Option<i64>,
) -> Result<RequestRecordSummary> {
    Ok(sqlx::query_file_as!(
        RequestRecordSummary,
        "src/sql/usage/usage_summary.sql",
        days,
        visible_user_id,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn list_request_records(
    pool: &PgPool,
    query: RequestRecordQuery,
) -> Result<RequestRecordPage> {
    let order_by = request_records_order_by_clause(&query.sort_field, query.sort_order);
    let rows = query.rows.clamp(1, 100);
    let first = query.first.max(0);
    let search = query.search.as_deref().filter(|value| !value.is_empty());
    let user = query.user.as_deref().filter(|value| !value.is_empty());
    let model = query.model.as_deref().filter(|value| !value.is_empty());
    let date_start = query.date_start;
    let date_end = query.date_end;
    let page_sql = REQUEST_RECORDS_PAGE_SQL.replace("/*__ORDER_BY__*/", order_by);

    let total = sqlx::query_file_scalar!(
        "src/sql/usage_events_count.sql",
        query.visible_user_id,
        query.request_category.as_str(),
        search,
        date_start,
        date_end,
        user,
        model,
        query.endpoint_id,
        query.mcp_server_id,
        query.mcp_bearer_token_slot,
        query.request_state.map(RequestRecordState::as_str),
        query.redaction_applied,
        query.client_key_id,
    )
    .fetch_one(pool)
    .await?;
    let records = sqlx::query_as::<_, RequestRecordListRowFlat>(sqlx::AssertSqlSafe(page_sql))
        .bind(query.visible_user_id)
        .bind(query.request_category.as_str())
        .bind(search)
        .bind(date_start)
        .bind(date_end)
        .bind(user)
        .bind(model)
        .bind(query.endpoint_id)
        .bind(query.mcp_server_id)
        .bind(query.mcp_bearer_token_slot)
        .bind(query.request_state.map(RequestRecordState::as_str))
        .bind(query.redaction_applied)
        .bind(query.client_key_id)
        .bind(rows)
        .bind(first)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(RequestRecordPage {
        total,
        records,
        first,
        rows,
    })
}

pub async fn list_request_record_facets(
    pool: &PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    is_admin: bool,
) -> Result<RequestRecordFacets> {
    let facets = sqlx::query_file_as!(
        crate::db::types::UsageFacet,
        "src/sql/usage_event_facets.sql",
        visible_user_id,
        request_category.as_str(),
    )
    .fetch_all(pool)
    .await?;
    let mut values = RequestRecordFacets::default();
    for facet in facets {
        match facet.facet.as_str() {
            "user" => values.users.push(facet.value),
            "model" => values.models.push(facet.value),
            "target" => values.models.push(facet.value),
            "date" => values.dates.push(facet.value),
            "client_key" => {
                let Some(key_id) = facet.key_id else {
                    continue;
                };
                values.client_keys.push(ClientKeyFacet {
                    key_id,
                    label: facet.label.unwrap_or(facet.value),
                    user_login_name: if is_admin {
                        facet.user_login_name
                    } else {
                        None
                    },
                });
            }
            _ => {}
        }
    }
    Ok(values)
}
