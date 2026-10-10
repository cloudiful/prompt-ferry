INSERT INTO chatgpt_quota_refresh_state AS state (
    endpoint_id,
    lease_owner,
    lease_expires_at,
    last_attempt_at
)
SELECT $1, $2, $4, $3
WHERE $4 > clock_timestamp()
  AND $4 <= clock_timestamp() + INTERVAL '60 seconds'
ON CONFLICT (endpoint_id) DO UPDATE
SET lease_owner = EXCLUDED.lease_owner,
    lease_expires_at = EXCLUDED.lease_expires_at,
    last_attempt_at = EXCLUDED.last_attempt_at
WHERE (state.lease_expires_at IS NULL OR state.lease_expires_at <= clock_timestamp())
  AND EXCLUDED.lease_expires_at > clock_timestamp()
  AND EXCLUDED.lease_expires_at <= clock_timestamp() + INTERVAL '60 seconds'
RETURNING endpoint_id;
