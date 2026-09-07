SELECT target_id, rule_id, endpoint_id, position, enabled, upstream_model
FROM standalone_model_route_targets
ORDER BY rule_id, position;
