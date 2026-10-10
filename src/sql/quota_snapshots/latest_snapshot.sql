SELECT
    snapshot_id,
    endpoint_id,
    observed_at,
    plan_type,
    limit_reached,
    windows,
    source,
    created_at
FROM chatgpt_quota_snapshots
WHERE endpoint_id = $1
ORDER BY snapshot_id DESC
LIMIT 1;
