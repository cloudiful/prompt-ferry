DELETE FROM mcp_oauth_leases
WHERE server_id = $1
  AND owner_user_id = $2
