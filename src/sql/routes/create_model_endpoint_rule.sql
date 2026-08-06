INSERT INTO model_endpoint_rules(
    scope,
    owner_user_id,
    model_pattern,
    routing_strategy,
    daily_max_requests,
    monthly_max_requests,
    endpoint_id,
    priority,
    enabled
)
VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8)
RETURNING rule_id, scope, owner_user_id, model_pattern, routing_strategy, daily_max_requests, monthly_max_requests, enabled, created_at, updated_at
