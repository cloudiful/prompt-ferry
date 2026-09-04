UPDATE provider_endpoints
SET
    scope = $2,
    owner_user_id = $3,
    name = $4,
    provider = $5,
    provider_region = $6,
    service_tier = $7,
    base_url = $8,
    native_api = $9,
    native_api_source = $10,
    daily_max_requests = $11,
    monthly_max_requests = $12,
    api_key = $13,
    key_lb_enabled = $14,
    enabled = $15,
    updated_at = NOW()
WHERE endpoint_id = $1
RETURNING endpoint_id, scope, owner_user_id, name, provider, provider_region, COALESCE(service_tier, 'standard') AS service_tier, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, mcp_enabled, created_at, updated_at
