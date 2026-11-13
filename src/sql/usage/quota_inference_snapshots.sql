SELECT observed_at, plan_type, limit_reached, windows
FROM chatgpt_quota_snapshots
WHERE endpoint_id = $1 AND observed_at >= $2 AND observed_at <= $3
ORDER BY observed_at ASC, snapshot_id ASC
LIMIT 30001
