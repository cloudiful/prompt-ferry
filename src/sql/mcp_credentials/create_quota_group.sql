INSERT INTO mcp_quota_groups (
    name, scope, owner_user_id, provider_kind, unit, daily_limit, monthly_limit,
    default_cost, strict_mode, billing_period_start, billing_period_end
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
RETURNING group_id, name, scope, owner_user_id, provider_kind, unit, daily_limit, monthly_limit,
          default_cost, strict_mode, billing_period_start, billing_period_end, created_at, updated_at
