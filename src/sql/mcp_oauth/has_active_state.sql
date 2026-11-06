SELECT EXISTS (
    SELECT 1
    FROM mcp_oauth_states
    WHERE server_id = $1
      AND owner_user_id = $2
      AND config_fingerprint = $3
      AND expires_at > NOW()
)
