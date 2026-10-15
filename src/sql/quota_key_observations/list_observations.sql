SELECT
    observation_id,
    endpoint_id,
    key_id,
    provider,
    key_label,
    source,
    observed_at,
    ok,
    status,
    error_code,
    plan_type,
    limit_reached,
    windows,
    created_at
FROM quota_key_observations
WHERE endpoint_id = $1
  AND ($2::UUID IS NULL OR key_id = $2)
  AND ($3::BIGINT IS NULL OR observation_id < $3)
ORDER BY observation_id DESC
LIMIT $4;
