SELECT endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, mcp_enabled, created_at, updated_at
FROM provider_endpoints
WHERE endpoint_id = $1
