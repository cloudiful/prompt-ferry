SELECT server_id,
    owner_user_id,
    connection_id,
    generation,
    lease_owner,
    expires_at
FROM standalone_mcp_oauth_leases
WHERE server_id = ?1
  AND owner_user_id = ?2
