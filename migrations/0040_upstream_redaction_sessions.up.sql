ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS upstream_redaction_enabled BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS upstream_redacted_request_json JSONB;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS restore_session_ciphertext BYTEA;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS restore_session_nonce BYTEA;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS restore_session_key_version SMALLINT;

CREATE TABLE IF NOT EXISTS conversation_redaction_sessions (
    conversation_id UUID PRIMARY KEY,
    session_ciphertext BYTEA NOT NULL,
    session_nonce BYTEA NOT NULL,
    session_key_version SMALLINT NOT NULL,
    last_event_id BIGINT REFERENCES request_records(event_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_conversation_redaction_sessions_last_event_id
ON conversation_redaction_sessions(last_event_id);
