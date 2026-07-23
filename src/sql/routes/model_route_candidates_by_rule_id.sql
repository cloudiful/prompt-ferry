SELECT r.rule_id, r.scope, r.owner_user_id, r.model_pattern, r.routing_strategy, r.session_affinity_lock_after_turns,
       r.daily_max_requests, r.monthly_max_requests, r.updated_at,
       t.target_id, e.endpoint_id, e.name AS endpoint_name, e.base_url, e.api_key, e.key_lb_enabled, e.native_api,
       t.position, t.enabled AS target_enabled, t.upstream_model, t.responses_continuation_policy
FROM model_endpoint_rules r
JOIN model_route_targets t ON t.rule_id = r.rule_id
JOIN provider_endpoints e ON e.endpoint_id = t.endpoint_id
WHERE r.rule_id = $1
  AND r.enabled = TRUE
  AND t.enabled = TRUE
  AND e.enabled = TRUE
ORDER BY
  CASE WHEN r.scope = 'user' THEN 0 ELSE 1 END,
  r.updated_at DESC,
  t.position ASC
