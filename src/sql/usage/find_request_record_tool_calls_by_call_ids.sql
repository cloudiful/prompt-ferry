SELECT
    tool_call.tool_call_event_id,
    tool_call.parent_event_id,
    tool_call.conversation_id,
    tool_call.call_id,
    tool_call.tool_name,
    tool_call.arguments_json,
    tool_call.arguments_preview,
    tool_call.status,
    tool_call.sequence_in_turn,
    tool_call.mcp_request_event_id,
    tool_call.created_at,
    tool_call.updated_at
FROM request_record_tool_calls AS tool_call
JOIN request_records AS parent
    ON parent.event_id = tool_call.parent_event_id
WHERE tool_call.call_id = ANY($1)
  AND parent.event_kind = 'request'
  AND parent.request_category = 'ai'
  AND parent.user_id IS NOT DISTINCT FROM $2::BIGINT
  AND parent.endpoint_id IS NOT DISTINCT FROM $3::UUID
ORDER BY tool_call.call_id ASC, tool_call.parent_event_id ASC,
         tool_call.sequence_in_turn ASC NULLS LAST,
         tool_call.tool_call_event_id ASC
