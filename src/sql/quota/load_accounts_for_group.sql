SELECT account_id, group_id, period_kind, period_start, period_end, used_units, reserved_units
FROM mcp_quota_accounts
WHERE group_id = $1 AND period_kind = $2 AND period_start = $3
