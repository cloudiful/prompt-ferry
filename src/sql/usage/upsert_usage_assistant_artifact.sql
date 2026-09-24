INSERT INTO request_record_assistant_artifacts(
    created_at, event_id, message_json, has_reasoning_content, has_tool_calls
)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (event_id, created_at) DO UPDATE
SET message_json = EXCLUDED.message_json,
    has_reasoning_content = EXCLUDED.has_reasoning_content,
    has_tool_calls = EXCLUDED.has_tool_calls
