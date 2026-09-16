UPDATE model_endpoint_rules
SET
    scope = $2,
    owner_user_id = $3,
    model_pattern = $4,
    routing_strategy = $5,
    endpoint_id = $6,
    priority = 0,
    enabled = $7,
    updated_at = NOW()
WHERE rule_id = $1
RETURNING rule_id, scope, owner_user_id, model_pattern, routing_strategy, enabled, created_at, updated_at
