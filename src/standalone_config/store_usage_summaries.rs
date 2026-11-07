use sqlx::Row;

use crate::standalone_config::{Result, StandaloneConfigError, StandaloneUsageSummaryRecord};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    // ---- Durable standalone usage summary ledger (Phase 1A). ----

    /// Persist a single compact standalone usage summary. The ledger key is
    /// the independent `event_id` column so retried, replayed, or repeated
    /// lifecycle events for the same request each become a distinct row.
    /// Returns the new row's `event_id`.
    pub async fn insert_usage_summary(
        &self,
        summary: &StandaloneUsageSummaryRecord,
    ) -> Result<i64> {
        let recorded_at = summary.recorded_at.to_rfc3339();
        let redaction_types_json = serde_json::to_string(&summary.redaction_types)?;
        let redaction_fields_json = serde_json::to_string(&summary.redaction_fields)?;
        let result = standalone_query!("src/sql/standalone/insert_usage_summary.sql")
            .bind(summary.request_id.to_string())
            .bind(&summary.event_kind)
            .bind(&summary.category)
            .bind(&summary.state)
            .bind(&summary.path)
            .bind(recorded_at)
            .bind(summary.status)
            .bind(summary.ok.map(i64::from))
            .bind(summary.duration_ms)
            .bind(summary.ttft_ms)
            .bind(&summary.model)
            .bind(&summary.requested_model)
            .bind(&summary.upstream_model)
            .bind(summary.endpoint_id.map(|id| id.to_string()))
            .bind(summary.endpoint_key_id.map(|id| id.to_string()))
            .bind(summary.model_route_rule_id.map(|id| id.to_string()))
            .bind(summary.mcp_server_id.map(|id| id.to_string()))
            .bind(summary.input_tokens)
            .bind(summary.output_tokens)
            .bind(summary.total_tokens)
            .bind(summary.cached_tokens)
            .bind(summary.cache_read_tokens)
            .bind(summary.cache_write_tokens)
            .bind(&summary.error_code)
            .bind(&summary.failure_family)
            .bind(i64::from(summary.redaction_applied))
            .bind(i64::from(summary.redaction_findings_count))
            .bind(i64::from(summary.redaction_replacements_count))
            .bind(redaction_types_json)
            .bind(redaction_fields_json)
            .bind(&summary.route_selection_reason)
            .bind(summary.user_id)
            .bind(summary.client_key_id)
            .bind(&summary.client_key_label)
            .bind(&summary.request_user_agent)
            .bind(&summary.endpoint_key_label)
            .bind(&summary.mcp_server_name)
            .bind(&summary.mcp_protocol_method)
            .bind(&summary.mcp_operation_name)
            .bind(&summary.http_request_content_encoding)
            .bind(i64::from(summary.http_request_compressed))
            .bind(summary.http_request_compressed_bytes)
            .bind(summary.http_request_decompressed_bytes)
            .bind(summary.http_request_compression_ratio)
            .bind(&summary.conversation_source)
            .bind(&summary.client_installation_id)
            .bind(&summary.session_header_id)
            .bind(&summary.session_parent_id)
            .bind(&summary.provider_response_id)
            .bind(&summary.provider_conversation_key)
            .bind(&summary.request_storage_mode)
            .bind(&summary.error_message)
            .bind(i64::from(summary.request_has_previous_response_id))
            .bind(&summary.request_previous_response_id)
            .bind(
                summary
                    .request_previous_response_parent_found
                    .map(i64::from),
            )
            .bind(&summary.request_conversation_key)
            .bind(summary.request_conversation_parent_found.map(i64::from))
            .bind(i64::from(summary.upstream_redaction_enabled))
            .bind(i64::from(summary.response_capture_truncated))
            .execute(&self.pool)
            .await?;
        Ok(result.last_insert_rowid())
    }

    /// Return at most `limit` recent summaries in insertion order (oldest
    /// first). `limit` is clamped to a positive value so a misconfigured
    /// caller cannot request a negative window. A single malformed row
    /// skips with a warning so the rest of the ledger still loads.
    pub async fn list_usage_summaries(
        &self,
        limit: i64,
    ) -> Result<Vec<StandaloneUsageSummaryRecord>> {
        let bounded = limit.clamp(1, 4096);
        let rows = standalone_query!("src/sql/standalone/list_usage_summaries.sql")
            .bind(bounded)
            .fetch_all(&self.pool)
            .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            match parse_usage_summary_row(&row) {
                Ok(Some(record)) => records.push(record),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "skipping malformed standalone usage summary row during list"
                    );
                }
            }
        }
        Ok(records)
    }

    /// Trim the standalone usage ledger so that at most `max_rows` rows
    /// remain. Older rows (by insertion order, expressed via `event_id`)
    /// are removed. Returns the number of rows deleted.
    pub async fn prune_usage_summaries(&self, max_rows: i64) -> Result<i64> {
        let bounded = max_rows.clamp(0, 4096);
        let result = standalone_query!("src/sql/standalone/prune_usage_summaries.sql")
            .bind(bounded)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as i64)
    }
}

/// Map a `standalone_usage_summaries` row into the storage DTO. Kept
/// private to this module so the low-level row layout stays internal to
/// the standalone config boundary.
///
/// Returns `Ok(None)` when the row's columns are individually well-typed
/// but carry a value that violates the domain (out-of-range integer,
/// malformed UUID, unparseable timestamp, etc.) so callers can keep the
/// rest of the ledger. Database-level failures still propagate as
/// `StandaloneConfigError::Database` so the surrounding query is not
/// silently retried against a corrupt connection.
fn parse_usage_summary_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<StandaloneUsageSummaryRecord>> {
    match try_parse_usage_summary_row(row) {
        Ok(record) => Ok(Some(record)),
        Err(StandaloneConfigError::CorruptDatabase(reason)) => {
            tracing::warn!(
                reason = %reason,
                "skipping malformed standalone usage summary row during list"
            );
            Ok(None)
        }
        Err(other) => Err(other),
    }
}

fn try_parse_usage_summary_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<StandaloneUsageSummaryRecord> {
    fn required_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String> {
        let value: String = row.try_get(column)?;
        if value.trim().is_empty() {
            return Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is empty"
            )));
        }
        Ok(value)
    }
    fn optional_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<String>> {
        Ok(row.try_get(column)?)
    }
    fn uuid_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<uuid::Uuid> {
        let value: String = row.try_get(column)?;
        uuid::Uuid::parse_str(&value).map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a UUID: {error}"
            ))
        })
    }
    fn uuid_optional(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<uuid::Uuid>> {
        let value = row.try_get::<Option<String>, _>(column)?;
        value
            .map(|value| {
                uuid::Uuid::parse_str(&value).map_err(|error| {
                    StandaloneConfigError::CorruptDatabase(format!(
                        "column {column} is not a UUID: {error}"
                    ))
                })
            })
            .transpose()
    }
    fn optional_i32(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<i32>> {
        let value = row.try_get::<Option<i64>, _>(column)?;
        value
            .map(|value| {
                i32::try_from(value).map_err(|_| {
                    StandaloneConfigError::CorruptDatabase(format!(
                        "column {column} is outside the i32 range"
                    ))
                })
            })
            .transpose()
    }
    fn optional_bool(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Option<bool>> {
        match row.try_get::<Option<i64>, _>(column)? {
            None => Ok(None),
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(other) => Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a boolean: {other}"
            ))),
        }
    }
    fn required_bool(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<bool> {
        match row.try_get::<i64, _>(column)? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a boolean: {value}"
            ))),
        }
    }
    fn parse_timestamp(value: &str, column: &str) -> Result<chrono::DateTime<chrono::Utc>> {
        if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
            return Ok(timestamp.with_timezone(&chrono::Utc));
        }
        let timestamp =
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map_err(|error| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "column {column} is not a timestamp: {error}"
                ))
            })?;
        Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
            timestamp,
            chrono::Utc,
        ))
    }
    fn sqlite_timestamp(
        row: &sqlx::sqlite::SqliteRow,
        column: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>> {
        let value: String = row.try_get(column)?;
        parse_timestamp(&value, column)
    }
    fn parse_string_list(value: &str, column: &str) -> Result<Vec<String>> {
        serde_json::from_str::<Vec<String>>(value).map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a string array: {error}"
            ))
        })
    }

    let redaction_types_json: String = row.try_get("redaction_types_json")?;
    let redaction_fields_json: String = row.try_get("redaction_fields_json")?;
    Ok(StandaloneUsageSummaryRecord {
        request_id: uuid_required(row, "request_id")?,
        event_kind: required_string(row, "event_kind")?,
        category: required_string(row, "category")?,
        state: required_string(row, "state")?,
        path: required_string(row, "path")?,
        recorded_at: sqlite_timestamp(row, "recorded_at")?,
        status: optional_i32(row, "status")?,
        ok: optional_bool(row, "ok")?,
        duration_ms: row.try_get("duration_ms")?,
        ttft_ms: row.try_get("ttft_ms")?,
        model: optional_string(row, "model")?,
        requested_model: optional_string(row, "requested_model")?,
        upstream_model: optional_string(row, "upstream_model")?,
        endpoint_id: uuid_optional(row, "endpoint_id")?,
        endpoint_key_id: uuid_optional(row, "endpoint_key_id")?,
        model_route_rule_id: uuid_optional(row, "model_route_rule_id")?,
        mcp_server_id: uuid_optional(row, "mcp_server_id")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cached_tokens: row.try_get("cached_tokens")?,
        cache_read_tokens: row.try_get("cache_read_tokens")?,
        cache_write_tokens: row.try_get("cache_write_tokens")?,
        error_code: optional_string(row, "error_code")?,
        failure_family: optional_string(row, "failure_family")?,
        redaction_applied: required_bool(row, "redaction_applied")?,
        redaction_findings_count: i32::try_from(row.try_get::<i64, _>("redaction_findings_count")?)
            .map_err(|_| {
                StandaloneConfigError::CorruptDatabase(
                    "redaction_findings_count is outside i32 range".to_string(),
                )
            })?,
        redaction_replacements_count: i32::try_from(
            row.try_get::<i64, _>("redaction_replacements_count")?,
        )
        .map_err(|_| {
            StandaloneConfigError::CorruptDatabase(
                "redaction_replacements_count is outside i32 range".to_string(),
            )
        })?,
        redaction_types: parse_string_list(&redaction_types_json, "redaction_types_json")?,
        redaction_fields: parse_string_list(&redaction_fields_json, "redaction_fields_json")?,
        route_selection_reason: required_string(row, "route_selection_reason")?,
        user_id: row.try_get("user_id")?,
        client_key_id: row.try_get("client_key_id")?,
        client_key_label: optional_string(row, "client_key_label")?,
        request_user_agent: optional_string(row, "request_user_agent")?,
        endpoint_key_label: optional_string(row, "endpoint_key_label")?,
        mcp_server_name: optional_string(row, "mcp_server_name")?,
        mcp_protocol_method: optional_string(row, "mcp_protocol_method")?,
        mcp_operation_name: optional_string(row, "mcp_operation_name")?,
        http_request_content_encoding: optional_string(row, "http_request_content_encoding")?,
        http_request_compressed: required_bool(row, "http_request_compressed")?,
        http_request_compressed_bytes: row.try_get("http_request_compressed_bytes")?,
        http_request_decompressed_bytes: row.try_get("http_request_decompressed_bytes")?,
        http_request_compression_ratio: row.try_get("http_request_compression_ratio")?,
        conversation_source: required_string(row, "conversation_source")?,
        client_installation_id: optional_string(row, "client_installation_id")?,
        session_header_id: optional_string(row, "session_header_id")?,
        session_parent_id: optional_string(row, "session_parent_id")?,
        provider_response_id: optional_string(row, "provider_response_id")?,
        provider_conversation_key: optional_string(row, "provider_conversation_key")?,
        request_storage_mode: required_string(row, "request_storage_mode")?,
        error_message: optional_string(row, "error_message")?,
        request_has_previous_response_id: required_bool(row, "request_has_previous_response_id")?,
        request_previous_response_id: optional_string(row, "request_previous_response_id")?,
        request_previous_response_parent_found: optional_bool(
            row,
            "request_previous_response_parent_found",
        )?,
        request_conversation_key: optional_string(row, "request_conversation_key")?,
        request_conversation_parent_found: optional_bool(row, "request_conversation_parent_found")?,
        upstream_redaction_enabled: required_bool(row, "upstream_redaction_enabled")?,
        response_capture_truncated: required_bool(row, "response_capture_truncated")?,
    })
}
