UPDATE request_records
SET created_at = $2,
    updated_at = $2
WHERE event_id = $1;
