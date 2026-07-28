DELETE FROM request_record_leases lease
WHERE NOT EXISTS (
    SELECT 1
    FROM request_records rr
    WHERE rr.request_id = lease.request_id
);
