INSERT INTO standalone_mcp_oauth_leases (
    server_id,
    owner_user_id,
    connection_id,
    generation,
    lease_owner,
    expires_at,
    updated_at
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT (server_id, owner_user_id) DO UPDATE
SET lease_owner = excluded.lease_owner,
    expires_at = excluded.expires_at,
    updated_at = excluded.updated_at
WHERE standalone_mcp_oauth_leases.connection_id = excluded.connection_id
  AND standalone_mcp_oauth_leases.generation = excluded.generation
  AND standalone_mcp_oauth_leases.expires_at <= ?8
