UPDATE request_record_content
SET created_at = $2
WHERE event_id = $1;
