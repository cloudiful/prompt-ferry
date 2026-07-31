WITH active AS (
    SELECT event_id, request_state
    FROM request_records
    WHERE request_id = $1
      AND event_kind = 'request'
      AND request_state IN ('received', 'awaiting_approval', 'upstream_processing')
    FOR UPDATE
)
UPDATE request_records rr
SET
    request_state = 'aborted',
    ok = FALSE,
    error_code = COALESCE(error_code, 'request_aborted'),
    error_message = COALESCE(error_message, $2),
    abort_reason = $3,
    abort_from_state = active.request_state,
    abort_response_started = $4,
    updated_at = NOW()
FROM active
WHERE rr.event_id = active.event_id
