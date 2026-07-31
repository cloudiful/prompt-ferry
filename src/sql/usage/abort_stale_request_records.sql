WITH stale AS (
    SELECT rr.event_id, rr.request_id, rr.request_state
    FROM request_records rr
    JOIN request_record_leases lease ON lease.request_id = rr.request_id
    WHERE rr.event_kind = 'request'
      AND rr.request_state IN ('received', 'awaiting_approval', 'upstream_processing')
      AND lease.lease_expires_at <= NOW()
), cleaned AS (
    DELETE FROM request_record_leases lease
    USING stale
    WHERE lease.request_id = stale.request_id
)
UPDATE request_records rr
SET
    request_state = 'aborted',
    ok = FALSE,
    error_code = COALESCE(rr.error_code, 'request_aborted'),
    error_message = COALESCE(
        rr.error_message,
        'request worker lease expired before completion; worker may have stopped or missed heartbeats'
    ),
    abort_reason = 'worker_lease_expired',
    abort_from_state = stale.request_state,
    abort_response_started = NULL,
    updated_at = NOW()
FROM stale
WHERE rr.event_id = stale.event_id
