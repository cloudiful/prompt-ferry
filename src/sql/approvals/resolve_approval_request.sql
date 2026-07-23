UPDATE approval_requests
SET
    approval_status = $2,
    request_payload_json = NULL,
    decided_by_user_id = $3,
    decided_at = CASE WHEN $3::BIGINT IS NULL THEN decided_at ELSE NOW() END,
    updated_at = NOW()
WHERE approval_id = $1
  AND approval_status = 'pending'
RETURNING
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
