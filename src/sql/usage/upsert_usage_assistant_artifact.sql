INSERT INTO request_record_assistant_artifacts(
    event_id, message_json, has_reasoning_content, has_tool_calls
)
VALUES ($1, $2, $3, $4)
ON CONFLICT (event_id) DO UPDATE
SET message_json = EXCLUDED.message_json,
    has_reasoning_content = EXCLUDED.has_reasoning_content,
    has_tool_calls = EXCLUDED.has_tool_calls
