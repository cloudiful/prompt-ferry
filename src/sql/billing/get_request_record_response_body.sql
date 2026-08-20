-- Single-row lookup helper used by the historical token backfill. Returns the
-- stored `response_raw_body` for one `(event_id, created_at)` pair, or no row
-- when the body has been pruned or migrated to object storage.
SELECT raw.response_raw_body AS "response_raw_body?"
FROM request_record_raw_payloads raw
WHERE raw.event_id = $1
  AND raw.created_at = $2
