INSERT INTO model_endpoint_rules(
    scope,
    owner_user_id,
    model_pattern,
    routing_strategy,
    endpoint_id,
    priority,
    enabled
)
VALUES ($1, $2, $3, $4, $5, 0, $6)
RETURNING rule_id, scope, owner_user_id, model_pattern, routing_strategy, enabled, created_at, updated_at
