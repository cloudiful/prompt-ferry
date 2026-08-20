-- Reads the canonical token columns for one `request_records` row so the
-- backfill integration tests can assert that a repair left the right values
-- behind.
SELECT
    input_tokens,
    output_tokens,
    total_tokens,
    cached_tokens,
    cache_read_tokens,
    cache_write_tokens
FROM request_records
WHERE event_id = $1
