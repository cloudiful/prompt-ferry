-- Return the most recently inserted summaries in insertion order (oldest
-- first by `event_id`, which is an alias for `rowid` on `INTEGER PRIMARY
-- KEY AUTOINCREMENT` tables). Callers bound the read with `LIMIT`; the
-- store also clamps the limit to a positive window before binding.
SELECT
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
FROM standalone_usage_summaries
ORDER BY event_id ASC
LIMIT ?;
