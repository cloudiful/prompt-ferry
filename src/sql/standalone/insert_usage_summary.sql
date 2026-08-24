-- Append a single compact standalone usage summary. The ledger key is the
-- independent `event_id` so retries, replays, and repeated lifecycle events
-- for the same request all become distinct rows; no natural uniqueness on
-- `request_id` is enforced here, matching the in-memory buffer behavior.
--
-- Phase 1C-a adds the non-secret request metadata columns introduced by
-- migration 0007. Raw request/response bodies, encrypted upstream sessions,
-- billing snapshots, approvals, and quota state are intentionally not
-- persisted in this table.
INSERT INTO standalone_usage_summaries (
    request_id, event_kind, category, state, path, recorded_at,
    status, ok, duration_ms, ttft_ms,
    model, requested_model, upstream_model,
    endpoint_id, endpoint_key_id, model_route_rule_id, mcp_server_id,
    input_tokens, output_tokens, total_tokens,
    cached_tokens, cache_read_tokens, cache_write_tokens,
    error_code, failure_family,
    redaction_applied, redaction_findings_count, redaction_replacements_count,
    redaction_types_json, redaction_fields_json,
    route_selection_reason,
    user_id, client_key_id, client_key_label, request_user_agent,
    endpoint_key_label,
    mcp_server_name, mcp_protocol_method, mcp_operation_name,
    http_request_content_encoding, http_request_compressed,
    http_request_compressed_bytes, http_request_decompressed_bytes,
    http_request_compression_ratio,
    conversation_source, client_installation_id,
    provider_response_id, provider_conversation_key,
    request_storage_mode, error_message,
    request_has_previous_response_id, request_previous_response_id,
    request_previous_response_parent_found,
    request_conversation_key, request_conversation_parent_found,
    upstream_redaction_enabled, response_capture_truncated
) VALUES (
    ?, ?, ?, ?, ?, ?,
    ?, ?, ?, ?,
    ?, ?, ?,
    ?, ?, ?, ?,
    ?, ?, ?,
    ?, ?, ?,
    ?, ?,
    ?, ?, ?,
    ?, ?,
    ?,
    ?, ?, ?, ?,
    ?,
    ?, ?, ?,
    ?, ?,
    ?, ?, ?,
    ?,
    ?, ?,
    ?, ?,
    ?,
    ?, ?, ?,
    ?, ?,
    ?, ?
);
