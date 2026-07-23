SELECT COUNT(*)::BIGINT AS count
FROM request_record_raw_payloads
WHERE event_id = $1;
