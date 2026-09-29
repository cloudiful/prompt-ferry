INSERT INTO standalone_endpoint_admin_keys (
    endpoint_id,
    admin_api_key_ciphertext, admin_api_key_nonce, admin_api_key_key_version,
    created_at, updated_at
) VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(endpoint_id) DO UPDATE SET
    admin_api_key_ciphertext = excluded.admin_api_key_ciphertext,
    admin_api_key_nonce = excluded.admin_api_key_nonce,
    admin_api_key_key_version = excluded.admin_api_key_key_version,
    updated_at = excluded.updated_at;
