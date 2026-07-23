SELECT event_id, endpoint_id, conversation_id, conversation_seq
FROM request_records
WHERE provider_conversation_key = $1
  AND user_id IS NOT DISTINCT FROM $2
  AND request_state = 'completed'
ORDER BY event_id DESC
LIMIT 1
