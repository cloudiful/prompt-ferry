SELECT
    tool_call_event_id,
    parent_event_id,
    conversation_id,
    call_id,
    tool_name,
    arguments_json,
    arguments_preview,
    status,
    sequence_in_turn,
    mcp_request_event_id,
    created_at,
    updated_at
FROM request_record_tool_calls
WHERE parent_event_id = $1
ORDER BY sequence_in_turn ASC NULLS LAST, tool_call_event_id ASC
