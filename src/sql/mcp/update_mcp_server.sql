UPDATE mcp_servers
SET
    scope = $2,
    owner_user_id = $3,
    name = $4,
    aggregate_naming_mode = $5,
    transport = $6,
    url = $7,
    command = $8,
    args = $9,
    env_json = $10,
    bearer_tokens_json = $11,
    http_headers_json = $12,
    tool_filter_mode = $13,
    allowed_tools = $14,
    disabled_tools = $15,
    disabled_resources = $16,
    daily_max_requests = $17,
    monthly_max_requests = $18,
    enabled = $19,
    timeout_ms = $20,
    updated_at = NOW()
WHERE server_id = $1
RETURNING server_id, scope, owner_user_id, name, aggregate_naming_mode, transport, url, command, args, env_json, bearer_tokens_json, http_headers_json, tool_filter_mode, allowed_tools, disabled_tools, disabled_resources, daily_max_requests, monthly_max_requests, enabled, timeout_ms, created_at, updated_at
