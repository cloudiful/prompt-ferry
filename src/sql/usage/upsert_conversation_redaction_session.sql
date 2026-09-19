INSERT INTO conversation_redaction_sessions(
    conversation_id,
    session_ciphertext,
    session_nonce,
    session_key_version,
    last_event_id,
    policy_version
)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (conversation_id)
DO UPDATE SET
    session_ciphertext = EXCLUDED.session_ciphertext,
    session_nonce = EXCLUDED.session_nonce,
    session_key_version = EXCLUDED.session_key_version,
    last_event_id = COALESCE(EXCLUDED.last_event_id, conversation_redaction_sessions.last_event_id),
    policy_version = EXCLUDED.policy_version,
    updated_at = NOW()
WHERE conversation_redaction_sessions.policy_version IS DISTINCT FROM EXCLUDED.policy_version
   OR EXCLUDED.last_event_id IS NULL
   OR conversation_redaction_sessions.last_event_id IS NULL
   OR conversation_redaction_sessions.last_event_id < EXCLUDED.last_event_id
