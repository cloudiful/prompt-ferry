WITH database_clock AS MATERIALIZED (
    SELECT clock_timestamp() AS db_now
)
INSERT INTO chatgpt_quota_refresh_state AS state (
    endpoint_id,
    lease_owner,
    lease_expires_at,
    last_attempt_at
)
SELECT $1, $2, database_clock.db_now + ($4::timestamptz - $3::timestamptz), $3
FROM database_clock
WHERE $4 > database_clock.db_now
  AND $4 > $3
  AND ($4 - $3) <= INTERVAL '60 seconds'
ON CONFLICT (endpoint_id) DO UPDATE
SET lease_owner = EXCLUDED.lease_owner,
    lease_expires_at = EXCLUDED.lease_expires_at,
    last_attempt_at = EXCLUDED.last_attempt_at
WHERE (state.lease_expires_at IS NULL OR state.lease_expires_at <= clock_timestamp())
  AND EXCLUDED.lease_expires_at > clock_timestamp()
RETURNING endpoint_id;
