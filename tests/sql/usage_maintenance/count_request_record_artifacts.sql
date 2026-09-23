SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_assistant_artifacts
WHERE event_id = $1;
