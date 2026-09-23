-- Issue #277 Phase P5: cheap idle gate for the metadata prune round.
-- The batch query's anti-joins are only worth paying for when something is
-- actually older than the retention cutoff; this probe rides the
-- `request_records.created_at` index instead.
SELECT EXISTS (
    SELECT 1
    FROM request_records rr
    WHERE rr.created_at < $1::TIMESTAMPTZ
) AS "has_expired!";
