DELETE FROM standalone_mcp_oauth_states
WHERE state_key_hash = ?1
  AND owner_user_id = ?2
  AND session_binding_hash = ?3
  AND config_fingerprint = ?4
  AND expires_at > ?5
RETURNING server_id,
    owner_user_id,
    session_binding_hash,
    connection_id,
    config_fingerprint,
    redirect_uri,
    expires_at,
    authorization_ciphertext,
    authorization_nonce,
    authorization_key_version,
    pending_client_ciphertext,
    pending_client_nonce,
    pending_client_key_version
