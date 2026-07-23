SELECT endpoint_id AS "route_id!", COALESCE(owner_user_id, 0) AS "user_id!", base_url, api_key, key_lb_enabled, native_api
FROM provider_endpoints
WHERE endpoint_id = $1
  AND enabled = TRUE
