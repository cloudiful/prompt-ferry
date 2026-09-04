WITH preferred AS (
    SELECT COALESCE(
        (SELECT endpoint_id FROM user_endpoint_settings WHERE user_id = $1),
        (SELECT endpoint_id
         FROM provider_endpoints
         WHERE scope = 'user'
           AND owner_user_id = $1
           AND enabled = TRUE
         ORDER BY updated_at DESC
         LIMIT 1),
        (SELECT endpoint_id
         FROM provider_endpoints
         WHERE scope = 'admin'
           AND owner_user_id IS NULL
           AND enabled = TRUE
         ORDER BY updated_at DESC
         LIMIT 1)
    ) AS endpoint_id
)
SELECT
    e.endpoint_id AS "route_id!",
    $1::BIGINT AS "user_id!",
    e.base_url,
    e.api_key,
    e.key_lb_enabled,
    e.native_api,
    e.provider,
    COALESCE(e.service_tier, 'standard') AS service_tier
FROM provider_endpoints e
CROSS JOIN preferred p
WHERE e.enabled = TRUE
  AND (
      (e.scope = 'user' AND e.owner_user_id = $1)
      OR (e.scope = 'admin' AND e.owner_user_id IS NULL)
  )
ORDER BY
    CASE
        WHEN e.endpoint_id = p.endpoint_id THEN 0
        WHEN e.scope = 'user' THEN 1
        ELSE 2
    END,
    e.updated_at DESC
