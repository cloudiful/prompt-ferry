SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_content
WHERE event_id = $1;
