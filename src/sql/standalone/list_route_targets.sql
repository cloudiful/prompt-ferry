SELECT target_id, rule_id, endpoint_id, position, enabled, upstream_model,
       native_api,
       proxy_url_override_ciphertext, proxy_url_override_nonce, proxy_url_override_key_version,
       active_windows, dev_system_normalize, thinking_effort_override
FROM standalone_model_route_targets
ORDER BY rule_id, position;
