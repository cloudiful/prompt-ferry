SELECT DISTINCT ON (e.endpoint_id) e.endpoint_id, e.base_url, e.api_key, e.key_lb_enabled, e.native_api, e.provider, COALESCE(e.service_tier, 'standard') AS service_tier
FROM model_endpoint_rules r
JOIN model_route_targets t ON t.rule_id = r.rule_id
JOIN provider_endpoints e ON e.endpoint_id = t.endpoint_id
WHERE r.enabled = TRUE
  AND t.enabled = TRUE
  AND e.enabled = TRUE
  AND (
      (r.scope = 'user' AND r.owner_user_id = $1)
      OR (r.scope = 'admin' AND r.owner_user_id IS NULL)
  )
ORDER BY
  e.endpoint_id,
  CASE WHEN r.scope = 'user' THEN 0 ELSE 1 END,
  t.position ASC,
  r.updated_at DESC
