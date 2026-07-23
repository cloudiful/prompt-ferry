DROP INDEX IF EXISTS idx_conversation_endpoint_overrides_endpoint_id;
DROP TABLE IF EXISTS conversation_endpoint_overrides;

DROP INDEX IF EXISTS idx_model_avoidances_avoid_until;
DROP TABLE IF EXISTS model_avoidances;

DROP INDEX IF EXISTS idx_usage_events_request_kind_created_at;

ALTER TABLE usage_events
DROP CONSTRAINT IF EXISTS ck_usage_events_event_kind;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS event_kind;
