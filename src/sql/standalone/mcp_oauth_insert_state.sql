INSERT INTO standalone_mcp_oauth_states (
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
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
ON CONFLICT (state_key_hash) DO UPDATE
SET server_id = excluded.server_id,
    owner_user_id = excluded.owner_user_id,
    session_binding_hash = excluded.session_binding_hash,
    connection_id = excluded.connection_id,
    config_fingerprint = excluded.config_fingerprint,
    redirect_uri = excluded.redirect_uri,
    authorization_ciphertext = excluded.authorization_ciphertext,
    authorization_nonce = excluded.authorization_nonce,
    authorization_key_version = excluded.authorization_key_version,
    pending_client_ciphertext = excluded.pending_client_ciphertext,
    pending_client_nonce = excluded.pending_client_nonce,
    pending_client_key_version = excluded.pending_client_key_version,
    expires_at = excluded.expires_at,
    created_at = excluded.created_at
