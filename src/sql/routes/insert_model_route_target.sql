INSERT INTO model_route_targets(
    rule_id,
    endpoint_id,
    position,
    enabled,
    upstream_model,
    responses_continuation_policy,
    chat_reasoning_replay_policy
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
