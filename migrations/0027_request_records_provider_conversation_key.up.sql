ALTER TABLE request_records
ADD COLUMN provider_conversation_key TEXT;

ALTER TABLE request_records
ADD COLUMN request_conversation_parent_found BOOLEAN;

CREATE INDEX IF NOT EXISTS idx_request_records_provider_conversation_key
ON request_records(provider_conversation_key, user_id, event_id DESC);
