WITH pending AS (
    SELECT reservation_id, day_account_id, month_account_id, units
    FROM mcp_quota_reservations
    WHERE status = 'reserved' AND expires_at < NOW()
    ORDER BY expires_at ASC
    LIMIT 500
),
marked AS (
    UPDATE mcp_quota_reservations r
    SET status = 'released',
        updated_at = NOW()
    FROM pending p
    WHERE r.reservation_id = p.reservation_id
    RETURNING r.day_account_id, r.month_account_id, p.units
)
UPDATE mcp_quota_accounts a
SET reserved_units = GREATEST(a.reserved_units - m.units, 0),
    updated_at = NOW()
FROM marked m
WHERE a.account_id = m.day_account_id OR a.account_id = m.month_account_id
