SELECT event_id, message_json, has_reasoning_content, has_tool_calls, created_at
FROM request_record_assistant_artifacts
WHERE event_id = ANY($1)
