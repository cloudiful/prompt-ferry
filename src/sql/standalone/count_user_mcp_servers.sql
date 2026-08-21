SELECT COUNT(*) AS total
FROM standalone_mcp_servers
WHERE scope = 'user' AND owner_user_id = ?;
