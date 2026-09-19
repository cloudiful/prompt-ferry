SELECT
    conversation_id,
    session_ciphertext,
    session_nonce,
    session_key_version,
    last_event_id,
    policy_version,
    created_at,
    updated_at
FROM conversation_redaction_sessions
WHERE conversation_id = $1
  AND policy_version = $2
