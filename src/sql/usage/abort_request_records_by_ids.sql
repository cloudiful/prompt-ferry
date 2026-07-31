WITH target AS (
    SELECT event_id, request_id, request_state
    FROM request_records
    WHERE event_kind = 'request'
      AND request_state IN ('received', 'awaiting_approval', 'upstream_processing')
      AND request_id = ANY($1::UUID[])
), cleaned AS (
    DELETE FROM request_record_leases lease
    USING target
    WHERE lease.request_id = target.request_id
)
UPDATE request_records rr
SET
    request_state = 'aborted',
    ok = FALSE,
    error_code = COALESCE(rr.error_code, 'request_aborted'),
    error_message = COALESCE(rr.error_message, 'request Valkey lease was missing before completion'),
    abort_reason = 'valkey_lease_missing',
    abort_from_state = target.request_state,
    abort_response_started = NULL,
    updated_at = NOW()
FROM target
WHERE rr.event_id = target.event_id
