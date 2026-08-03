SELECT t.target_id, t.rule_id, t.endpoint_id, e.name AS endpoint_name,
       COALESCE(e.enabled, FALSE) AS "endpoint_enabled!",
       t.position, t.enabled, t.upstream_model, t.responses_continuation_policy,
       t.chat_reasoning_replay_policy, t.created_at, t.updated_at
FROM model_route_targets t
LEFT JOIN provider_endpoints e ON e.endpoint_id = t.endpoint_id
WHERE t.rule_id = ANY($1)
ORDER BY t.rule_id ASC, t.position ASC, t.created_at ASC
