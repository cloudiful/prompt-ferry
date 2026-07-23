SELECT event_id,
       user_id,
       conversation_id AS "conversation_id!: Uuid",
       conversation_seq AS "conversation_seq!: i32",
       provider_response_id AS "provider_response_id!: String"
FROM request_records
WHERE event_kind = 'request'
ORDER BY created_at DESC
LIMIT 1
