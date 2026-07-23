UPDATE provider_endpoints
SET
    scope = $2,
    owner_user_id = $3,
    name = $4,
    base_url = $5,
    native_api = $6,
    native_api_source = $7,
    daily_max_requests = $8,
    monthly_max_requests = $9,
    api_key = $10,
    key_lb_enabled = $11,
    enabled = $12,
    updated_at = NOW()
WHERE endpoint_id = $1
RETURNING endpoint_id, scope, owner_user_id, name, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, created_at, updated_at
