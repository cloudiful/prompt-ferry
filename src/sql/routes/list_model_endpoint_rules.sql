SELECT rule_id, scope, owner_user_id, model_pattern, routing_strategy, session_affinity_lock_after_turns, daily_max_requests, monthly_max_requests, enabled, created_at, updated_at
FROM model_endpoint_rules
ORDER BY
  CASE WHEN scope = 'user' THEN 0 ELSE 1 END,
  owner_user_id NULLS FIRST,
  model_pattern ASC,
  updated_at DESC
