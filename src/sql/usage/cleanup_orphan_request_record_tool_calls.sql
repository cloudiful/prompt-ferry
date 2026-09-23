DELETE FROM request_record_tool_calls calls
USING request_records parent
WHERE parent.event_id = calls.parent_event_id
  AND NOT EXISTS (
      SELECT 1
      FROM request_record_content content
      WHERE content.event_id = parent.event_id
  )
