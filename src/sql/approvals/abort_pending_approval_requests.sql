UPDATE approval_requests
SET
    approval_status = 'aborted',
    request_payload_json = NULL,
    updated_at = NOW()
WHERE approval_status = 'pending'
