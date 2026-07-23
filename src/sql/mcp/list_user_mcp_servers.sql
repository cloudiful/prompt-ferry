SELECT server_id, scope, owner_user_id, name, aggregate_naming_mode, transport, url, command, args, env_json, bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools, disabled_tools, disabled_resources, daily_max_requests, monthly_max_requests, enabled, timeout_ms, created_at, updated_at
FROM mcp_servers
WHERE scope = 'user'
  AND owner_user_id = $1
ORDER BY name ASC
