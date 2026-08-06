SELECT rule_id, scope, owner_user_id, model_pattern, routing_strategy, daily_max_requests, monthly_max_requests, enabled, created_at, updated_at
FROM model_endpoint_rules
WHERE rule_id = $1
