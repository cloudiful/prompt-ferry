-- Prune standalone usage summaries so that at most :max_rows rows remain.
-- The oldest rows (by `event_id`, which is the insertion-order surrogate
-- key) are deleted first; subsequent inserts grow the ledger back from the
-- highest surviving `event_id`.
DELETE FROM standalone_usage_summaries
WHERE event_id IN (
    SELECT event_id
    FROM standalone_usage_summaries
    ORDER BY event_id ASC
    LIMIT MAX(0, (SELECT COUNT(*) FROM standalone_usage_summaries) - ?1)
);
