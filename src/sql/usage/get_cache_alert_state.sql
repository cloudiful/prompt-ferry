SELECT last_alerted_at AS "last_alerted_at!"
FROM cache_alert_state
WHERE conversation_id = $1
