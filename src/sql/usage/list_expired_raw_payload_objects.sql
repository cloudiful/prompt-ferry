SELECT event_id, created_at, raw_object_key AS "raw_object_key!"
FROM request_record_raw_payloads
WHERE created_at < $1
  AND raw_object_key IS NOT NULL
  AND (created_at > $2 OR (created_at = $2 AND event_id > $3))
ORDER BY created_at ASC, event_id ASC
LIMIT $4;
