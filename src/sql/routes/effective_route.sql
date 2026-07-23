WITH selected AS (
    SELECT COALESCE(
        (SELECT endpoint_id FROM user_endpoint_settings WHERE user_id = $1),
        (SELECT endpoint_id FROM provider_endpoints WHERE scope = 'user' AND owner_user_id = $1 AND enabled = TRUE ORDER BY updated_at DESC LIMIT 1),
        (SELECT endpoint_id FROM provider_endpoints WHERE scope = 'admin' AND enabled = TRUE ORDER BY updated_at DESC LIMIT 1)
    ) AS endpoint_id
)
SELECT e.endpoint_id AS "route_id!", $1::BIGINT AS "user_id!", e.base_url, e.api_key, e.key_lb_enabled, e.native_api
FROM selected s
JOIN provider_endpoints e ON e.endpoint_id = s.endpoint_id
WHERE e.enabled = TRUE
