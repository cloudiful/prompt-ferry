SELECT rule_id, scope, owner_user_id, model_pattern, routing_strategy, enabled
FROM standalone_model_routes
WHERE rule_id = ?;