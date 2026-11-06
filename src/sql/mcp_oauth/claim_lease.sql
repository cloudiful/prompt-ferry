INSERT INTO mcp_oauth_leases (
    server_id,
    owner_user_id,
    connection_id,
    generation,
    lease_owner,
    expires_at,
    updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, NOW())
ON CONFLICT (server_id, owner_user_id) DO UPDATE
SET lease_owner = EXCLUDED.lease_owner,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
WHERE mcp_oauth_leases.connection_id = EXCLUDED.connection_id
  AND mcp_oauth_leases.generation = EXCLUDED.generation
  AND mcp_oauth_leases.expires_at <= NOW()
RETURNING server_id,
    owner_user_id,
    connection_id,
    generation,
    lease_owner,
    expires_at
