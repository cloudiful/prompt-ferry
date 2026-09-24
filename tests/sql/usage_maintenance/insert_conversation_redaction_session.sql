INSERT INTO conversation_redaction_sessions (
    conversation_id,
    session_ciphertext,
    session_nonce,
    session_key_version,
    last_event_id,
    created_at,
    updated_at
)
VALUES ($1, '\x00'::bytea, '\x00'::bytea, 1, $2, $3, $3)
ON CONFLICT (conversation_id) DO UPDATE
SET last_event_id = EXCLUDED.last_event_id,
    updated_at = EXCLUDED.updated_at;
