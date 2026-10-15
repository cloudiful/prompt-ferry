WITH expired AS (
    SELECT observation_id
    FROM quota_key_observations
    WHERE observed_at < $1
    ORDER BY observed_at, observation_id
    LIMIT $2
    FOR UPDATE SKIP LOCKED
)
DELETE FROM quota_key_observations AS observations
USING expired
WHERE observations.observation_id = expired.observation_id;
