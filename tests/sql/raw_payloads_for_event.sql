-- Raw bodies no longer live in PostgreSQL (migration 0066); the body columns
-- are kept in this helper's contract as typed NULLs so callers observe the
-- object-store-backed behavior, while conversation metadata stays authoritative.
SELECT
    CAST(NULL AS JSONB) AS request_raw_json,
    CAST(NULL AS TEXT) AS response_raw_body,
    rr.request_conversation_key
FROM request_records rr
LEFT JOIN request_record_raw_payloads raw
  ON raw.event_id = rr.event_id
 AND raw.created_at = rr.created_at
WHERE rr.event_id = $1;
