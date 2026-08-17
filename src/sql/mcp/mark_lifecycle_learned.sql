UPDATE mcp_servers
SET lifecycle_learned_mode = $2,
    lifecycle_learned_protocol_version = $3,
    lifecycle_learned_for_updated_at = updated_at,
    lifecycle_learned_at = NOW()
WHERE server_id = $1
  AND updated_at = $4
