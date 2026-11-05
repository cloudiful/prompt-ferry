-- One latest observation per (selected key, relative bucket) inside the
-- half-open request window. `bucket_index` is the relative index from `from`
-- with `bucket_seconds` steps; the representative row is chosen from identity
-- and timing columns only, then the primary-key join reads the winning JSON.
-- The row_number tie-break keeps the newest observed row (observation_id
-- further breaks an exact timestamp tie) so a failed/unknown latest row blanks
-- that bucket instead of falling back to an older success.
WITH bucketed AS (
    SELECT
        observation.observation_id,
        observation.key_id,
        observation.observed_at,
        FLOOR(
            EXTRACT(EPOCH FROM (observation.observed_at - $2::TIMESTAMPTZ))
                / $5::BIGINT
        )::BIGINT AS bucket_index
    FROM quota_key_observations AS observation
    WHERE observation.endpoint_id = $1
      AND observation.observed_at >= $2
      AND observation.observed_at < $3
      AND observation.key_id = ANY($4::UUID[])
),
ranked AS (
    SELECT
        observation_id,
        key_id,
        bucket_index,
        ROW_NUMBER() OVER (
            PARTITION BY key_id, bucket_index
            ORDER BY observed_at DESC, observation_id DESC
        ) AS bucket_rank
    FROM bucketed
)
SELECT
    observation.key_id,
    observation.key_label,
    observation.observed_at,
    observation.ok,
    observation.windows,
    ranked.bucket_index AS "bucket_index!"
FROM ranked
JOIN quota_key_observations AS observation
    ON observation.observation_id = ranked.observation_id
WHERE ranked.bucket_rank = 1
ORDER BY observation.observed_at DESC, observation.observation_id DESC;
