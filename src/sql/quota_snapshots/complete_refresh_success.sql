UPDATE chatgpt_quota_refresh_state
SET lease_owner = NULL,
    lease_expires_at = NULL,
    last_success_at = $3,
    consecutive_failures = 0,
    next_retry_at = NULL,
    last_error_code = NULL
WHERE endpoint_id = $1
  AND lease_owner = $2
  AND lease_expires_at > clock_timestamp()
RETURNING endpoint_id;
