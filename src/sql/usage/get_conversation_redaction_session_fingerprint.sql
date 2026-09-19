SELECT
    policy_version,
    session_key_version,
    last_event_id,
    sha256(session_ciphertext) AS "session_ciphertext_sha256!"
FROM conversation_redaction_sessions
WHERE conversation_id = $1
