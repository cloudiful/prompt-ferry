-- Marks a `request_records` row as having a truncated response capture so the
-- backfill candidate SQL surfaces it via `response_capture_truncated`. Used
-- by the truncated-guard regression test.
UPDATE request_records
SET response_capture_truncated = $2
WHERE event_id = $1
