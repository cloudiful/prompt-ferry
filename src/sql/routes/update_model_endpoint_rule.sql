UPDATE model_endpoint_rules
SET
    scope = $2,
    owner_user_id = $3,
    model_pattern = $4,
    routing_strategy = $5,
    daily_max_requests = $6,
    monthly_max_requests = $7,
    endpoint_id = $8,
    priority = 0,
    enabled = $9,
    updated_at = NOW()
WHERE rule_id = $1
RETURNING rule_id, scope, owner_user_id, model_pattern, routing_strategy, daily_max_requests, monthly_max_requests, enabled, created_at, updated_at
