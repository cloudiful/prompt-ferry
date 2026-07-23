DROP INDEX IF EXISTS idx_usage_events_conversation_created_at;
DROP INDEX IF EXISTS idx_usage_events_codex_session_candidates;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS conversation_source;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS normalized_last_ref_hash;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS normalized_first_ref_hash;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS normalized_chain_hash;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS normalized_item_count;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS client_installation_id;
