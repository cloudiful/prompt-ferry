INSERT INTO chatgpt_quota_snapshots (
    endpoint_id,
    observed_at,
    plan_type,
    limit_reached,
    windows,
    source
)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING
    snapshot_id,
    endpoint_id,
    observed_at,
    plan_type,
    limit_reached,
    windows,
    source,
    created_at;
