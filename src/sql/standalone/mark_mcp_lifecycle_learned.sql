UPDATE standalone_mcp_servers
SET lifecycle_learned_mode = ?,
    lifecycle_learned_protocol_version = ?,
    lifecycle_learned_for_updated_at = updated_at,
    lifecycle_learned_at = ?
WHERE server_id = ?
  AND updated_at = ?;
