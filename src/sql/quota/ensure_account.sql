INSERT INTO mcp_quota_accounts (group_id, period_kind, period_start, period_end)
VALUES ($1, $2, $3, $4)
ON CONFLICT (group_id, period_kind, period_start) DO NOTHING
