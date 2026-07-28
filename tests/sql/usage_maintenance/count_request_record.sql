SELECT COUNT(*)::BIGINT AS "count!"
FROM request_records
WHERE event_id = $1;
