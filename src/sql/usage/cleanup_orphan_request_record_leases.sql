-- Issue #277 Phase P8: request_record_leases is a plain table outside the
-- partition lifecycle, so dropping metadata partitions can leave leases that
-- point at a request id no longer present. Bounded, index-backed cleanup for
-- the 15-minute tick.
DELETE FROM request_record_leases lease
WHERE NOT EXISTS (
    SELECT 1
    FROM request_records rr
    WHERE rr.request_id = lease.request_id
);
