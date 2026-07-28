WITH candidates AS MATERIALIZED (
    SELECT approval_id
    FROM approval_requests
    WHERE approval_status <> 'pending'
      AND created_at < $1::TIMESTAMPTZ
    ORDER BY created_at ASC, approval_id ASC
    LIMIT $2::BIGINT
    FOR UPDATE SKIP LOCKED
), deleted AS (
    DELETE FROM approval_requests approvals
    USING candidates
    WHERE approvals.approval_id = candidates.approval_id
    RETURNING approvals.approval_id
)
SELECT COUNT(*)::BIGINT AS "deleted_count!"
FROM deleted;
