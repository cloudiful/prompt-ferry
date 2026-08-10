INSERT INTO mcp_quota_reservations (
    day_account_id, month_account_id, credential_id, request_id, units, expires_at
)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING reservation_id, day_account_id, month_account_id, credential_id, request_id, units
