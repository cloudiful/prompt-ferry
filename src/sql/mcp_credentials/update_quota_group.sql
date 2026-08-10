UPDATE mcp_quota_groups
SET name = $2,
    scope = $3,
    owner_user_id = $4,
    provider_kind = $5,
    unit = $6,
    daily_limit = $7,
    monthly_limit = $8,
    default_cost = $9,
    strict_mode = $10,
    billing_period_start = $11,
    billing_period_end = $12,
    updated_at = NOW()
WHERE group_id = $1
RETURNING group_id, name, scope, owner_user_id, provider_kind, unit, daily_limit, monthly_limit,
          default_cost, strict_mode, billing_period_start, billing_period_end, created_at, updated_at
