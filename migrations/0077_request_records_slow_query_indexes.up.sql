-- P0 (issue #277): `request_records` reached 569k rows / ~640 MB heap and the
-- usage overview, facets, and in-flight lease sweeps were degrading into
-- parallel seq scans (2-3.8s). Two indexes stop the bleeding:
--
-- 1. Covering index for the overview/facets readers. `metrics.sql` selected
--    `rr.*`, which materialized all ~90 columns (including the JSONB request
--    payloads) and made any index-only plan impossible. The SQL now projects
--    only the columns below, so this index can serve an Index Only Scan
--    instead of touching the wide heap row.
-- 2. Partial index over the in-flight request states, so
--    `list_active_request_record_ids.sql` no longer seq-scans the table on
--    every lease sweep (that scan was the 2.49s statement that stalled the
--    single-row lease writes).
CREATE INDEX IF NOT EXISTS idx_request_records_usage_covering
ON request_records (request_category, created_at DESC)
INCLUDE (
    user_id,
    ok,
    request_state,
    duration_ms,
    ttft_ms,
    input_tokens,
    output_tokens,
    total_tokens,
    cached_tokens,
    cache_read_tokens,
    cache_write_tokens,
    mcp_protocol_method,
    model,
    endpoint_id,
    failure_family,
    mcp_server_id,
    mcp_server_name,
    client_key_id,
    client_key_label
)
WHERE event_kind = 'request';

CREATE INDEX IF NOT EXISTS idx_request_records_inflight_request_id
ON request_records (request_id)
WHERE event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing');
