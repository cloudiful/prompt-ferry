-- One latest observation per (selected key, relative bucket) inside the
-- half-open request window. `bucket_index` is the relative index from `from`
-- with `bucket_seconds` steps; the row_number tie-break keeps the newest
-- observed row (observation_id further breaks an exact timestamp tie) so a
-- failed/unknown latest row blanks that bucket instead of falling back to an
-- older success.
WITH selected_keys AS (
    SELECT unnest($4::UUID[]) AS key_id
),
ranked AS (
    SELECT
        observation.observation_id,
        observation.key_id,
        observation.key_label,
        observation.observed_at,
        observation.ok,
        observation.windows,
        FLOOR(
            EXTRACT(EPOCH FROM (observation.observed_at - $2::TIMESTAMPTZ))
                / $5::BIGINT
        )::BIGINT AS bucket_index,
        ROW_NUMBER() OVER (
            PARTITION BY
                observation.key_id,
                FLOOR(
                    EXTRACT(EPOCH FROM (observation.observed_at - $2::TIMESTAMPTZ))
                        / $5::BIGINT
                )
            ORDER BY observation.observed_at DESC, observation.observation_id DESC
        ) AS bucket_rank
    FROM quota_key_observations AS observation
    JOIN selected_keys ON selected_keys.key_id = observation.key_id
    WHERE observation.endpoint_id = $1
      AND observation.observed_at >= $2
      AND observation.observed_at < $3
)
SELECT
    key_id,
    key_label,
    observed_at,
    ok,
    windows,
    bucket_index AS "bucket_index!"
FROM ranked
WHERE bucket_rank = 1
ORDER BY observed_at DESC, observation_id DESC;
