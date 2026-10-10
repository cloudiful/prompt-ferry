WITH expired AS (
    SELECT snapshot_id
    FROM chatgpt_quota_snapshots
    WHERE observed_at < $1
    ORDER BY observed_at, snapshot_id
    LIMIT $2
    FOR UPDATE SKIP LOCKED
)
DELETE FROM chatgpt_quota_snapshots AS snapshots
USING expired
WHERE snapshots.snapshot_id = expired.snapshot_id;
