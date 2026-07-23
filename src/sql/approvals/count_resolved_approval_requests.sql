SELECT COUNT(*)::BIGINT AS "total!"
FROM approval_requests
WHERE approval_status <> 'pending'
