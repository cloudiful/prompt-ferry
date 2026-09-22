INSERT INTO cache_alert_state (
    conversation_id,
    last_alerted_at,
    last_cache_rate,
    last_turns
)
VALUES ($1, NOW(), $2, $3)
ON CONFLICT (conversation_id)
DO UPDATE SET
    last_alerted_at = EXCLUDED.last_alerted_at,
    last_cache_rate = EXCLUDED.last_cache_rate,
    last_turns = EXCLUDED.last_turns
