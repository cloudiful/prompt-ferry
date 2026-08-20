-- Updates only the token fields of a request_record from a canonical parse of
-- the retained upstream raw payload. Each token column uses
-- `COALESCE($n, existing)` so a missing parsed field never overwrites a
-- stored value. Only `Some(0)` and other authoritative parsed values
-- propagate; NULL parsed values fall through to the existing row. The
-- `updated_at` column is refreshed on every write so the operator can audit
-- when the row last moved through the backfill path.
UPDATE request_records
SET input_tokens = COALESCE($2, input_tokens),
    output_tokens = COALESCE($3, output_tokens),
    total_tokens = COALESCE($4, total_tokens),
    cached_tokens = COALESCE($5, cached_tokens),
    cache_read_tokens = COALESCE($6, cache_read_tokens),
    cache_write_tokens = COALESCE($7, cache_write_tokens),
    updated_at = NOW()
WHERE event_id = $1
