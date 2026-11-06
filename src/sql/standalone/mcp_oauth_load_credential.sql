SELECT server_id,
    owner_user_id,
    connection_id,
    generation,
    config_fingerprint,
    registration_ciphertext,
    registration_nonce,
    registration_key_version,
    credentials_ciphertext,
    credentials_nonce,
    credentials_key_version,
    scopes,
    expires_at,
    issuer,
    created_at,
    updated_at
FROM standalone_mcp_oauth_credentials
WHERE server_id = ?1
  AND owner_user_id = ?2
