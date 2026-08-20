-- Scans AI request_records that still retain raw upstream usage for the historical
-- token-normalization backfill. The candidate set is bounded by `--limit`, an
-- optional time window, and a cursor `after_event_id` so the CLI can advance
-- the cursor across batches without re-reading rows. Only `completed` rows are
-- surfaced because the backfill target is historical successful requests;
-- `failed` rows often have zero or partial usage and must not be silently
-- overwritten. Records without a raw payload in PostgreSQL
-- (`response_raw_body` and `request_raw_json`) but with an object-store reference
-- (`raw_object_key`) are still surfaced so the caller can report them as skipped
-- rather than missing them silently. The `response_capture_truncated` flag is
-- surfaced so the caller can refuse to overwrite rows whose upstream capture
-- was truncated and therefore carries incomplete usage.
SELECT
    rr.event_id,
    rr.request_id,
    rr.created_at,
    rr.requested_model,
    rr.upstream_model,
    rr.model AS model,
    rr.input_tokens AS existing_input_tokens,
    rr.output_tokens AS existing_output_tokens,
    rr.total_tokens AS existing_total_tokens,
    rr.cached_tokens AS existing_cached_tokens,
    rr.cache_read_tokens AS existing_cache_read_tokens,
    rr.cache_write_tokens AS existing_cache_write_tokens,
    raw.response_raw_body IS NOT NULL AS "response_in_postgres!",
    raw.request_raw_json IS NOT NULL AS "request_in_postgres!",
    raw.raw_object_key IS NOT NULL AS "raw_object_only!",
    COALESCE(rr.response_capture_truncated, FALSE) AS "response_capture_truncated!"
FROM request_records rr
LEFT JOIN request_record_raw_payloads raw
  ON raw.event_id = rr.event_id
 AND raw.created_at = rr.created_at
WHERE rr.event_kind = 'request'
  AND rr.request_category = 'ai'
  AND rr.request_state = 'completed'
  AND rr.event_id > $4
  AND ($1::TIMESTAMPTZ IS NULL OR rr.created_at >= $1)
  AND ($2::TIMESTAMPTZ IS NULL OR rr.created_at < $2)
ORDER BY rr.event_id ASC
LIMIT $3
