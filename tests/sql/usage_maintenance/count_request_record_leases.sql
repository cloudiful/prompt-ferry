SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_leases
WHERE request_id = $1;
