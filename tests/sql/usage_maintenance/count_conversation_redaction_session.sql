SELECT COUNT(*)::BIGINT AS "count!"
FROM conversation_redaction_sessions
WHERE conversation_id = $1;
