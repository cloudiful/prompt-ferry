SELECT event_id, endpoint_id, conversation_id, conversation_seq
FROM request_records records
WHERE conversation_id = $1
  AND user_id IS NOT DISTINCT FROM $2
  AND request_state = 'completed'
  AND EXISTS (
      SELECT 1
      FROM request_record_content content
      WHERE content.event_id = records.event_id
  )
ORDER BY conversation_seq DESC NULLS LAST, event_id DESC
LIMIT 1
