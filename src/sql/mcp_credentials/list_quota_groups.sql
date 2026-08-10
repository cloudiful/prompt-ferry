SELECT group_id, name, scope, owner_user_id, provider_kind, unit, daily_limit, monthly_limit,
       default_cost, strict_mode, billing_period_start, billing_period_end, created_at, updated_at
FROM mcp_quota_groups
ORDER BY name ASC, created_at ASC
