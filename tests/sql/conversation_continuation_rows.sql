SELECT
    event_id,
    parent_event_id,
    conversation_seq AS "conversation_seq!",
    request_state AS "request_state!"
FROM request_records
WHERE path = '/v1/responses'
  AND request_conversation_key = $1
ORDER BY event_id ASC
LIMIT 3
