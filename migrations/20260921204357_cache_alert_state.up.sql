-- Issue #548 Task 1: per-conversation fingerprint of the last cache-rate
-- alert. `last_alerted_at` is the cooldown clock (one row per conversation),
-- so a conversation that keeps a low cache rate is only re-alerted after the
-- configured cooldown. PostgreSQL only; standalone SQLite keeps no alert
-- state and does not aggregate cache-rate alerts.
CREATE TABLE IF NOT EXISTS cache_alert_state (
    conversation_id UUID PRIMARY KEY,
    last_alerted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_cache_rate DOUBLE PRECISION NOT NULL,
    last_turns INTEGER NOT NULL
);
