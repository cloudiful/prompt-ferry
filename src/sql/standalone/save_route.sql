INSERT INTO standalone_model_routes (
    rule_id, scope, owner_user_id, model_pattern, routing_strategy,
    enabled, updated_at
) VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(rule_id) DO UPDATE SET
    scope = excluded.scope,
    owner_user_id = excluded.owner_user_id,
    model_pattern = excluded.model_pattern,
    routing_strategy = excluded.routing_strategy,
    enabled = excluded.enabled,
    updated_at = CURRENT_TIMESTAMP;
