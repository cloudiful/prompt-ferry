UPDATE mcp_quota_reservations r
SET status = $2,
    committed_at = CASE WHEN $2 = 'committed' THEN NOW() ELSE NULL END,
    updated_at = NOW()
WHERE r.request_id = $1
  AND r.status = 'reserved'
  AND r.reservation_id = (
      SELECT reservation_id
      FROM mcp_quota_reservations
      WHERE request_id = $1 AND status = 'reserved'
      ORDER BY reservation_id
      LIMIT 1
  )
RETURNING r.reservation_id, r.day_account_id, r.month_account_id, r.credential_id, r.request_id, r.units
