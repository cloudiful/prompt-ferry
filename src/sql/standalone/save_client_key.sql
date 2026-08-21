INSERT INTO standalone_client_keys (
    key_id, user_id, key_prefix, label, enabled,
    secret_ciphertext, secret_nonce, secret_key_version, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(key_id) DO UPDATE SET
    user_id = excluded.user_id,
    key_prefix = excluded.key_prefix,
    label = excluded.label,
    enabled = excluded.enabled,
    secret_ciphertext = excluded.secret_ciphertext,
    secret_nonce = excluded.secret_nonce,
    secret_key_version = excluded.secret_key_version,
    updated_at = CURRENT_TIMESTAMP;
