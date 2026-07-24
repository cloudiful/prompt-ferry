SELECT endpoint_id, scope, owner_user_id, name, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, created_at, updated_at
FROM provider_endpoints
ORDER BY updated_at DESC, endpoint_id DESC
OFFSET $1
LIMIT $2
