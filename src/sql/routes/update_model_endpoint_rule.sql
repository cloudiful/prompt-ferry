UPDATE model_endpoint_rules
SET
    scope = $2,
    owner_user_id = $3,
    model_pattern = $4,
    routing_strategy = $5,
    session_affinity_lock_after_turns = $6,
    daily_max_requests = $7,
    monthly_max_requests = $8,
    endpoint_id = $9,
    priority = 0,
    enabled = $10,
    updated_at = NOW()
WHERE rule_id = $1
RETURNING rule_id, scope, owner_user_id, model_pattern, routing_strategy, session_affinity_lock_after_turns, daily_max_requests, monthly_max_requests, enabled, created_at, updated_at
