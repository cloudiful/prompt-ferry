UPDATE request_record_raw_payloads
SET created_at = NOW() - INTERVAL '2 days'
WHERE event_id = $1;
