UPDATE request_records
SET
    request_state = 'aborted',
    ok = FALSE,
    error_code = COALESCE(error_code, 'request_aborted'),
    error_message = COALESCE(error_message, $2),
    updated_at = NOW()
WHERE request_id = $1
  AND event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing')
