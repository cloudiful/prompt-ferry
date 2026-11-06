SELECT server_id,
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
FROM standalone_mcp_oauth_states
WHERE state_key_hash = ?1
  AND server_id = ?2
  AND owner_user_id = ?3
  AND session_binding_hash = ?4
  AND config_fingerprint = ?5
  AND expires_at > ?6
