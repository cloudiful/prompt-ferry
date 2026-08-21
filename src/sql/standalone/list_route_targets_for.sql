SELECT target_id, rule_id, endpoint_id, position, enabled, upstream_model,
       responses_continuation_policy
FROM standalone_model_route_targets
WHERE rule_id = ?
ORDER BY position;