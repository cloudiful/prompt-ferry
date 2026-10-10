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
  AND observed_at >= NOW() - INTERVAL '30 days'
  AND ($2::BIGINT IS NULL OR snapshot_id < $2)
ORDER BY snapshot_id DESC
LIMIT $3;
