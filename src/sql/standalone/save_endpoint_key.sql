INSERT INTO standalone_endpoint_keys (
    key_id, endpoint_id, key_label, enabled, position,
    api_key_ciphertext, api_key_nonce, api_key_key_version, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(key_id) DO UPDATE SET
    endpoint_id = excluded.endpoint_id,
    key_label = excluded.key_label,
    enabled = excluded.enabled,
    position = excluded.position,
    api_key_ciphertext = excluded.api_key_ciphertext,
    api_key_nonce = excluded.api_key_nonce,
    api_key_key_version = excluded.api_key_key_version,
    updated_at = CURRENT_TIMESTAMP;
