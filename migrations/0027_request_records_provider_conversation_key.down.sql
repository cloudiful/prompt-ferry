DROP INDEX IF EXISTS idx_request_records_provider_conversation_key;

ALTER TABLE request_records
DROP COLUMN IF EXISTS request_conversation_parent_found;

ALTER TABLE request_records
DROP COLUMN IF EXISTS provider_conversation_key;
