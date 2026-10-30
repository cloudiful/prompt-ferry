INSERT INTO cache_alert_state (
    conversation_id,
    channel,
    last_alerted_at,
    last_cache_rate,
    last_turns
)
VALUES ($1, $2, NOW(), $3, $4)
ON CONFLICT (conversation_id, channel)
DO UPDATE SET
    last_alerted_at = EXCLUDED.last_alerted_at,
    last_cache_rate = EXCLUDED.last_cache_rate,
    last_turns = EXCLUDED.last_turns
