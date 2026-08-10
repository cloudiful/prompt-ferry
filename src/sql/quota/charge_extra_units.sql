UPDATE mcp_quota_accounts a
SET used_units = a.used_units + $2,
    updated_at = NOW()
WHERE a.account_id = $1
RETURNING a.account_id, a.group_id, a.period_kind, a.period_start, a.period_end,
          a.used_units, a.reserved_units
