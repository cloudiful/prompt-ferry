DELETE FROM mcp_oauth_credentials
WHERE server_id = $1
  AND owner_user_id = $2
