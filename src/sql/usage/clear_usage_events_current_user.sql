WITH candidates AS MATERIALIZED (
    SELECT rr.event_id
    FROM request_records rr
    WHERE rr.user_id = $1
      AND ($2::TIMESTAMPTZ IS NULL OR rr.created_at >= $2)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at <= $3)
), protected AS (
    SELECT event_id
    FROM candidates
    WHERE EXISTS (
        SELECT 1
        FROM usage_charges charge
        WHERE charge.event_id = candidates.event_id
    )
), deleted AS (
    DELETE FROM request_records rr
    USING candidates
    WHERE rr.event_id = candidates.event_id
      AND NOT EXISTS (
          SELECT 1
          FROM usage_charges charge
          WHERE charge.event_id = rr.event_id
      )
    RETURNING rr.event_id
)
SELECT
    (SELECT COUNT(*) FROM deleted)::BIGINT AS "deleted_count!",
    (SELECT COUNT(*) FROM protected)::BIGINT AS "protected_by_billing!";
