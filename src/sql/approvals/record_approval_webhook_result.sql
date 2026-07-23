UPDATE approval_requests
SET
    webhook_last_error = $2,
    webhook_last_attempted_at = NOW(),
    updated_at = NOW()
WHERE approval_id = $1
