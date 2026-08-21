INSERT INTO standalone_model_route_targets (
    target_id, rule_id, endpoint_id, position, enabled, upstream_model,
    responses_continuation_policy, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(target_id) DO UPDATE SET
    rule_id = excluded.rule_id,
    endpoint_id = excluded.endpoint_id,
    position = excluded.position,
    enabled = excluded.enabled,
    upstream_model = excluded.upstream_model,
    responses_continuation_policy = excluded.responses_continuation_policy,
    updated_at = CURRENT_TIMESTAMP;
