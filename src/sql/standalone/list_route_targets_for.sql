SELECT target_id, rule_id, endpoint_id, position, enabled, upstream_model,
       proxy_url_override_ciphertext, proxy_url_override_nonce, proxy_url_override_key_version
FROM standalone_model_route_targets
WHERE rule_id = ?
ORDER BY position;