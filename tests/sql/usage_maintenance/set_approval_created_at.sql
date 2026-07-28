UPDATE approval_requests
SET created_at = $2,
    updated_at = $2
WHERE approval_id = $1;
