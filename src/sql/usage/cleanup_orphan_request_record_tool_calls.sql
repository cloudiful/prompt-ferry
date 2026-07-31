DELETE FROM request_record_tool_calls calls
USING request_records parent
WHERE parent.event_id = calls.parent_event_id
  AND parent.content_expired_at IS NOT NULL
