INSERT INTO standalone_endpoint_oauth_tokens (
    endpoint_id,
    access_token_ciphertext, access_token_nonce, access_token_key_version,
    refresh_token_ciphertext, refresh_token_nonce, refresh_token_key_version,
    expires_at,
    created_at, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(endpoint_id) DO UPDATE SET
    access_token_ciphertext = excluded.access_token_ciphertext,
    access_token_nonce = excluded.access_token_nonce,
    access_token_key_version = excluded.access_token_key_version,
    refresh_token_ciphertext = excluded.refresh_token_ciphertext,
    refresh_token_nonce = excluded.refresh_token_nonce,
    refresh_token_key_version = excluded.refresh_token_key_version,
    expires_at = excluded.expires_at,
    updated_at = excluded.updated_at;
