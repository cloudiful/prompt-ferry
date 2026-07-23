ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS client_installation_id TEXT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS normalized_item_count INT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS normalized_chain_hash TEXT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS normalized_first_ref_hash TEXT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS normalized_last_ref_hash TEXT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS conversation_source TEXT NOT NULL DEFAULT 'none';

CREATE INDEX IF NOT EXISTS idx_usage_events_codex_session_candidates
ON usage_events(user_id, path, client_installation_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_usage_events_conversation_created_at
ON usage_events(conversation_id, created_at DESC);
