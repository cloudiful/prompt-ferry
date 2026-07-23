INSERT INTO request_record_tool_calls (
    parent_event_id,
    conversation_id,
    call_id,
    tool_name,
    arguments_json,
    arguments_preview,
    status,
    sequence_in_turn,
    mcp_request_event_id
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
ON CONFLICT (parent_event_id, call_id)
DO UPDATE SET
    conversation_id = COALESCE(EXCLUDED.conversation_id, request_record_tool_calls.conversation_id),
    tool_name = EXCLUDED.tool_name,
    arguments_json = COALESCE(EXCLUDED.arguments_json, request_record_tool_calls.arguments_json),
    arguments_preview = COALESCE(EXCLUDED.arguments_preview, request_record_tool_calls.arguments_preview),
    status = EXCLUDED.status,
    sequence_in_turn = COALESCE(EXCLUDED.sequence_in_turn, request_record_tool_calls.sequence_in_turn),
    mcp_request_event_id = COALESCE(EXCLUDED.mcp_request_event_id, request_record_tool_calls.mcp_request_event_id),
    updated_at = NOW()
RETURNING
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
