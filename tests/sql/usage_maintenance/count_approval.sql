SELECT COUNT(*)::BIGINT AS "count!"
FROM approval_requests
WHERE approval_id = $1;
