CREATE INDEX IF NOT EXISTS idx_request_record_tool_calls_call_parent
ON request_record_tool_calls(call_id, parent_event_id);
