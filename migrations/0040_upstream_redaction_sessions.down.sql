DROP INDEX IF EXISTS idx_conversation_redaction_sessions_last_event_id;

DROP TABLE IF EXISTS conversation_redaction_sessions;

ALTER TABLE request_records
DROP COLUMN IF EXISTS restore_session_key_version;

ALTER TABLE request_records
DROP COLUMN IF EXISTS restore_session_nonce;

ALTER TABLE request_records
DROP COLUMN IF EXISTS restore_session_ciphertext;

ALTER TABLE request_records
DROP COLUMN IF EXISTS upstream_redacted_request_json;

ALTER TABLE request_records
DROP COLUMN IF EXISTS upstream_redaction_enabled;
