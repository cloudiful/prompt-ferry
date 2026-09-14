INSERT INTO standalone_model_route_targets (
    target_id, rule_id, endpoint_id, position, enabled, upstream_model,
    proxy_url_override_ciphertext, proxy_url_override_nonce, proxy_url_override_key_version,
    active_windows,
    updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(target_id) DO UPDATE SET
    rule_id = excluded.rule_id,
    endpoint_id = excluded.endpoint_id,
    position = excluded.position,
    enabled = excluded.enabled,
    upstream_model = excluded.upstream_model,
    proxy_url_override_ciphertext = excluded.proxy_url_override_ciphertext,
    proxy_url_override_nonce = excluded.proxy_url_override_nonce,
    proxy_url_override_key_version = excluded.proxy_url_override_key_version,
    active_windows = excluded.active_windows,
    updated_at = CURRENT_TIMESTAMP;
