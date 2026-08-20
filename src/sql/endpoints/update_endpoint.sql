UPDATE provider_endpoints
SET
    scope = $2,
    owner_user_id = $3,
    name = $4,
    provider = $5,
    provider_region = $6,
    base_url = $7,
    native_api = $8,
    native_api_source = $9,
    daily_max_requests = $10,
    monthly_max_requests = $11,
    api_key = $12,
    key_lb_enabled = $13,
    enabled = $14,
    updated_at = NOW()
WHERE endpoint_id = $1
RETURNING endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, mcp_enabled, created_at, updated_at
