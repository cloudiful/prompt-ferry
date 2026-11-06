INSERT INTO mcp_oauth_states (
    state_key_hash,
    server_id,
    owner_user_id,
    session_binding_hash,
    connection_id,
    config_fingerprint,
    redirect_uri,
    authorization_ciphertext,
    authorization_nonce,
    authorization_key_version,
    pending_client_ciphertext,
    pending_client_nonce,
    pending_client_key_version,
    expires_at,
    created_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NOW())
ON CONFLICT (state_key_hash) DO UPDATE
SET server_id = EXCLUDED.server_id,
    owner_user_id = EXCLUDED.owner_user_id,
    session_binding_hash = EXCLUDED.session_binding_hash,
    connection_id = EXCLUDED.connection_id,
    config_fingerprint = EXCLUDED.config_fingerprint,
    redirect_uri = EXCLUDED.redirect_uri,
    authorization_ciphertext = EXCLUDED.authorization_ciphertext,
    authorization_nonce = EXCLUDED.authorization_nonce,
    authorization_key_version = EXCLUDED.authorization_key_version,
    pending_client_ciphertext = EXCLUDED.pending_client_ciphertext,
    pending_client_nonce = EXCLUDED.pending_client_nonce,
    pending_client_key_version = EXCLUDED.pending_client_key_version,
    expires_at = EXCLUDED.expires_at,
    created_at = NOW()
