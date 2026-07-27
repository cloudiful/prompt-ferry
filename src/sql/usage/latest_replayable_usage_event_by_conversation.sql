SELECT event_id, endpoint_id, conversation_id, conversation_seq
FROM request_records
WHERE conversation_id = $1
  AND user_id IS NOT DISTINCT FROM $2
  AND request_state = 'completed'
  AND content_expired_at IS NULL
ORDER BY conversation_seq DESC NULLS LAST, event_id DESC
LIMIT 1
