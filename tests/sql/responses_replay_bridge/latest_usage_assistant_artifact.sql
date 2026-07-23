SELECT has_reasoning_content AS "has_reasoning_content!",
       has_tool_calls AS "has_tool_calls!"
FROM request_record_assistant_artifacts
ORDER BY created_at DESC
LIMIT 1
