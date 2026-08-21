SELECT server_id, source_endpoint_id, scope, owner_user_id, name,
       aggregate_naming_mode, transport, url, command, args_json,
       http_headers_json, tool_filter_mode, allowed_tools_json,
       disabled_tools_json, disabled_resources_json, daily_max_requests,
       monthly_max_requests, enabled, timeout_ms, lifecycle_policy,
       lifecycle_manual_protocol_version, lifecycle_learned_mode,
       lifecycle_learned_protocol_version, lifecycle_learned_for_updated_at,
       lifecycle_learned_at, env_ciphertext, env_nonce, env_key_version,
       bearer_tokens_ciphertext, bearer_tokens_nonce, bearer_tokens_key_version,
       created_at, updated_at
FROM standalone_mcp_servers
ORDER BY scope DESC, owner_user_id IS NOT NULL, owner_user_id, name;
