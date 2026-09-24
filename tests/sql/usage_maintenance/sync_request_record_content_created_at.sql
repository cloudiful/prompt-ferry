UPDATE request_record_content
SET created_at = (SELECT created_at FROM request_records WHERE event_id = $1)
WHERE event_id = $1;
