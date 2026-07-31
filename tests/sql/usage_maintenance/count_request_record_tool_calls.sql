SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_tool_calls
WHERE parent_event_id = $1
