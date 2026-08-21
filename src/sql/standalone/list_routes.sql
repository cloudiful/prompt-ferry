SELECT rule_id, scope, owner_user_id, model_pattern, routing_strategy,
       daily_max_requests, monthly_max_requests, enabled
FROM standalone_model_routes
ORDER BY model_pattern, rule_id;
