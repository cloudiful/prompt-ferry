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
    api_key = $11,
    proxy_url = $12,
    active_windows = $13,
    key_lb_enabled = $14,
    enabled = $15,
    updated_at = NOW()
WHERE endpoint_id = $1
RETURNING endpoint_id, scope, owner_user_id, name, provider, provider_region, COALESCE(service_tier, 'standard') AS service_tier, base_url, native_api, native_api_source, api_key, proxy_url, active_windows, key_lb_enabled, enabled, mcp_enabled, created_at, updated_at
