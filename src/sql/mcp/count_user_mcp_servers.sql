SELECT COUNT(*)::BIGINT AS "total!"
FROM mcp_servers
WHERE scope = 'user'
  AND owner_user_id = $1
