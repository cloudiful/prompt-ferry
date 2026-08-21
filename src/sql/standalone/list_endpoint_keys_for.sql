SELECT key_id, endpoint_id, key_label, enabled, position,
       api_key_ciphertext, api_key_nonce, api_key_key_version,
       created_at, updated_at
FROM standalone_endpoint_keys
WHERE endpoint_id = ?
ORDER BY position;