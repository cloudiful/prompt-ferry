SELECT endpoint_id,
       access_token_ciphertext, access_token_nonce, access_token_key_version,
       refresh_token_ciphertext, refresh_token_nonce, refresh_token_key_version,
       expires_at
FROM standalone_endpoint_oauth_tokens
WHERE endpoint_id = ?;
