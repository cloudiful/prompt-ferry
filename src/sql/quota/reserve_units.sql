UPDATE mcp_quota_accounts a
SET reserved_units = a.reserved_units + $4,
    updated_at = NOW()
WHERE a.group_id = $1
  AND a.period_kind = $2
  AND a.period_start = $3
  AND ($5::double precision IS NULL OR a.used_units + a.reserved_units + $4 <= $5)
RETURNING a.account_id, a.group_id, a.period_kind, a.period_start, a.period_end,
          a.used_units, a.reserved_units
