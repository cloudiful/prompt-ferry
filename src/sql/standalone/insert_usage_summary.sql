-- Append a single compact standalone usage summary. The ledger key is the
-- independent `event_id` so retries, replays, and repeated lifecycle events
-- for the same request all become distinct rows; no natural uniqueness on
-- `request_id` is enforced here, matching the in-memory buffer behavior.
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
    route_selection_reason
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
    ?
);
