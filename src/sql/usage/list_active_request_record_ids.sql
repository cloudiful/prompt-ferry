SELECT request_id
FROM request_records
WHERE event_kind = 'request'
  AND request_state IN ('received', 'upstream_processing')
  AND request_id IS NOT NULL
