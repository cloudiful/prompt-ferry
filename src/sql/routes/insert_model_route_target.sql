INSERT INTO model_route_targets(
    rule_id,
    endpoint_id,
    position,
    enabled,
    upstream_model,
    native_api,
    proxy_url_override,
    active_windows,
    dev_system_normalize,
    thinking_effort_override
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
