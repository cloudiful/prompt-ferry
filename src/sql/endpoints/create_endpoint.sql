INSERT INTO provider_endpoints(scope, owner_user_id, name, provider, provider_region, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
RETURNING endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, native_api, native_api_source, daily_max_requests, monthly_max_requests, api_key, key_lb_enabled, enabled, created_at, updated_at
