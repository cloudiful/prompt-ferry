UPDATE chatgpt_quota_refresh_state
SET lease_owner = NULL,
    lease_expires_at = NULL,
    last_attempt_at = $3,
    consecutive_failures = CASE
        WHEN consecutive_failures < 2147483647 THEN consecutive_failures + 1
        ELSE consecutive_failures
    END,
    next_retry_at = $4,
    last_error_code = $5
WHERE endpoint_id = $1
  AND lease_owner = $2
  AND lease_expires_at > clock_timestamp()
RETURNING endpoint_id;
