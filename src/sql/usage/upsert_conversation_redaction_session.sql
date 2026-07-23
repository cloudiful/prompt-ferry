INSERT INTO conversation_redaction_sessions(
    conversation_id,
    session_ciphertext,
    session_nonce,
    session_key_version,
    last_event_id
)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (conversation_id)
DO UPDATE SET
    session_ciphertext = EXCLUDED.session_ciphertext,
    session_nonce = EXCLUDED.session_nonce,
    session_key_version = EXCLUDED.session_key_version,
    last_event_id = COALESCE(EXCLUDED.last_event_id, conversation_redaction_sessions.last_event_id),
    updated_at = NOW()
