SELECT server_id,
    owner_user_id,
    session_binding_hash,
    connection_id,
    config_fingerprint,
    redirect_uri,
    expires_at
FROM mcp_oauth_states
WHERE state_key_hash = $1
  AND expires_at > NOW()
