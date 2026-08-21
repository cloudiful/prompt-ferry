UPDATE standalone_provider_endpoints
SET mcp_enabled = ?, updated_at = ?
WHERE endpoint_id = ?;
