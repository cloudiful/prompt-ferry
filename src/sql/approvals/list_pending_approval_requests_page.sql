SELECT
    approval_id,
    request_id,
    user_id,
    (SELECT login_name FROM users WHERE user_id = approval_requests.user_id) AS "user_login_name?",
    client_key_label,
    path,
    model,
    review_decision,
    approval_status,
    review_reason,
    review_categories,
    request_preview,
    request_payload_json,
    request_deadline_unix_ms,
    wait_deadline_unix_ms,
    decided_by_user_id,
    (SELECT login_name FROM users WHERE user_id = approval_requests.decided_by_user_id) AS "decided_by_login_name?",
    decided_at,
    created_at,
    updated_at
FROM approval_requests
WHERE approval_status = 'pending'
ORDER BY wait_deadline_unix_ms ASC NULLS LAST, created_at DESC, approval_id ASC
OFFSET $1
LIMIT $2
