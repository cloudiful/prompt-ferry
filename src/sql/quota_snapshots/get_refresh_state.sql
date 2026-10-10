SELECT
    endpoint_id,
    lease_owner,
    lease_expires_at,
    last_attempt_at,
    last_success_at,
    consecutive_failures,
    next_retry_at,
    last_error_code
FROM chatgpt_quota_refresh_state
WHERE endpoint_id = $1;
