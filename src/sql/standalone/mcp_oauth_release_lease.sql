DELETE FROM standalone_mcp_oauth_leases
WHERE server_id = ?1
  AND owner_user_id = ?2
  AND connection_id = ?3
  AND generation = ?4
  AND lease_owner = ?5
