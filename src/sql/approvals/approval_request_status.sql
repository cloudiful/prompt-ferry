SELECT approval_status, decided_at
FROM approval_requests
WHERE approval_id = $1
