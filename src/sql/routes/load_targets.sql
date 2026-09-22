SELECT t.target_id, t.rule_id, t.endpoint_id, e.name AS endpoint_name,
       COALESCE(e.enabled, FALSE) AS "endpoint_enabled!",
       t.position, t.enabled, t.upstream_model,
       COALESCE(t.native_api, 'auto') AS "native_api!",
        t.proxy_url_override,
        t.active_windows,
        COALESCE(t.dev_system_normalize, FALSE) AS "dev_system_normalize!",
        COALESCE(t.thinking_downgrade_enabled, FALSE) AS "thinking_downgrade_enabled!",
        t.thinking_effort_override,
        t.compact_mode,
       t.created_at, t.updated_at
FROM model_route_targets t
LEFT JOIN provider_endpoints e ON e.endpoint_id = t.endpoint_id
WHERE t.rule_id = ANY($1)
ORDER BY t.rule_id ASC, t.position ASC, t.created_at ASC
