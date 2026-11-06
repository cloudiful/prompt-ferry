INSERT INTO standalone_mcp_oauth_credentials (
    server_id,
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
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
ON CONFLICT (server_id, owner_user_id) DO UPDATE
SET connection_id = excluded.connection_id,
    generation = excluded.generation,
    config_fingerprint = excluded.config_fingerprint,
    registration_ciphertext = excluded.registration_ciphertext,
    registration_nonce = excluded.registration_nonce,
    registration_key_version = excluded.registration_key_version,
    credentials_ciphertext = excluded.credentials_ciphertext,
    credentials_nonce = excluded.credentials_nonce,
    credentials_key_version = excluded.credentials_key_version,
    scopes = excluded.scopes,
    expires_at = excluded.expires_at,
    issuer = excluded.issuer,
    updated_at = excluded.updated_at
