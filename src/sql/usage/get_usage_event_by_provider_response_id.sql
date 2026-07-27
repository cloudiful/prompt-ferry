SELECT event_id, endpoint_id, conversation_id, conversation_seq
FROM request_records
WHERE provider_response_id = $1
  AND user_id IS NOT DISTINCT FROM $2
  AND content_expired_at IS NULL
ORDER BY event_id DESC
LIMIT 1
