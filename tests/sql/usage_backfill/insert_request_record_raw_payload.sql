-- Inserts a retained raw response body for a single `request_records` row.
-- Used by the backfill integration tests to attach a synthetic upstream
-- payload so the historical repair can be exercised end-to-end without a
-- real upstream call.
INSERT INTO request_record_raw_payloads(event_id, created_at, request_raw_json, response_raw_body)
SELECT $1, created_at, $2, $3
FROM request_records
WHERE event_id = $1
ON CONFLICT (created_at, event_id) DO UPDATE SET
    request_raw_json = COALESCE(
        EXCLUDED.request_raw_json,
        request_record_raw_payloads.request_raw_json
    ),
    response_raw_body = COALESCE(
        EXCLUDED.response_raw_body,
        request_record_raw_payloads.response_raw_body
    )
