WITH candidates AS MATERIALIZED (
    SELECT rr.event_id
    FROM request_records rr
    WHERE rr.created_at < $1::TIMESTAMPTZ
      AND rr.request_state NOT IN ('received', 'awaiting_approval', 'upstream_processing')
      AND (rr.lease_expires_at IS NULL OR rr.lease_expires_at <= NOW())
      AND NOT EXISTS (
          SELECT 1
          FROM request_record_leases lease
          WHERE lease.request_id = rr.request_id
            AND lease.lease_expires_at > NOW()
      )
      AND NOT EXISTS (
          SELECT 1
          FROM usage_charges charge
          WHERE charge.event_id = rr.event_id
      )
    ORDER BY rr.created_at ASC, rr.event_id ASC
    LIMIT $2::BIGINT
    FOR UPDATE OF rr SKIP LOCKED
), deleted AS (
    DELETE FROM request_records rr
    USING candidates
    WHERE rr.event_id = candidates.event_id
    RETURNING rr.event_id
), orphan_leases AS (
    DELETE FROM request_record_leases lease
    WHERE NOT EXISTS (
        SELECT 1
        FROM request_records rr
        WHERE rr.request_id = lease.request_id
    )
    RETURNING lease.request_id
)
SELECT
    (SELECT COUNT(*) FROM deleted)::BIGINT AS "deleted_count!",
    (SELECT COUNT(*) FROM orphan_leases)::BIGINT AS "orphan_leases_deleted!";
