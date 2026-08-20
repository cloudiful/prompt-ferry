UPDATE provider_endpoints
SET mcp_enabled = $2,
    updated_at = NOW()
WHERE endpoint_id = $1
