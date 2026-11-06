DELETE FROM standalone_mcp_oauth_states
WHERE server_id = ?1
  AND owner_user_id = ?2
