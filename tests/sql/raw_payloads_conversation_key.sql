SELECT request_conversation_key
FROM request_records
WHERE event_id = $1;
