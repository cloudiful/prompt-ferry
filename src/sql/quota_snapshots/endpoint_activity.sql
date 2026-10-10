SELECT
    endpoint_id AS "endpoint_id!",
    MAX(created_at) AS "last_activity_at!"
FROM request_records
WHERE endpoint_id = ANY($1)
  AND created_at >= $2
  AND created_at >= NOW() - INTERVAL '30 minutes'
GROUP BY endpoint_id
ORDER BY endpoint_id;
