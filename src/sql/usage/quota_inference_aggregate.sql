WITH intervals AS (
    SELECT starts.at AS start_at, ends.at AS end_at, starts.ordinality AS interval_index
    FROM unnest($4::timestamptz[]) WITH ORDINALITY AS starts(at, ordinality)
    JOIN unnest($5::timestamptz[]) WITH ORDINALITY AS ends(at, ordinality)
      USING (ordinality)
), source AS (
    SELECT rr.*,
           COALESCE(NULLIF(BTRIM(rr.upstream_model), ''), NULLIF(BTRIM(rr.model), '')) AS effective_model,
           rr.duration_ms IS NOT NULL AND rr.duration_ms BETWEEN 0 AND 86400000 AS valid_duration,
           CASE WHEN rr.duration_ms BETWEEN 0 AND 86400000
                THEN rr.created_at + rr.duration_ms * INTERVAL '1 millisecond'
                ELSE rr.created_at END AS attributed_at,
           rr.ok IS TRUE AND rr.request_state IS NOT DISTINCT FROM 'completed' AS successful,
           (rr.input_tokens IS NOT NULL OR rr.output_tokens IS NOT NULL OR
            rr.cache_read_tokens IS NOT NULL OR rr.cached_tokens IS NOT NULL OR
            rr.cache_write_tokens IS NOT NULL) AS has_usage,
           CASE WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                     AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                     AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
                THEN GREATEST(COALESCE(rr.input_tokens, 0) - GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0) - GREATEST(COALESCE(rr.cache_write_tokens, 0), 0), 0)
                ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0) END::bigint AS fresh,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::bigint AS cache_read,
           COALESCE(rr.cache_write_tokens, 0)::bigint AS cache_write,
           COALESCE(rr.output_tokens, 0)::bigint AS output
    FROM request_records rr
    WHERE rr.endpoint_id = $1 AND rr.event_kind = 'request' AND rr.request_category = 'ai'
      AND rr.created_at >= $2::timestamptz - INTERVAL '24 hours' AND rr.created_at <= $3
), ranked AS (
    SELECT effective_model, row_number() OVER (ORDER BY SUM(fresh + cache_read + cache_write + output) DESC, effective_model ASC) AS rank
    FROM source, intervals i
    WHERE i.interval_index = 1 AND attributed_at > i.start_at AND attributed_at <= i.end_at
      AND valid_duration AND successful AND has_usage AND effective_model IS NOT NULL
    GROUP BY effective_model
), buckets AS (
    SELECT s.*, CASE WHEN s.effective_model IS NULL THEN 'unknown'
                     WHEN r.rank <= 32 THEN 'model' ELSE 'overflow' END AS bucket,
           CASE WHEN r.rank <= 32 THEN s.effective_model END AS bucket_model
    FROM source s LEFT JOIN ranked r ON r.effective_model = s.effective_model
), grouped AS (
    SELECT i.interval_index, b.bucket, b.bucket_model AS model,
           COUNT(*) FILTER (WHERE b.valid_duration AND b.successful AND b.has_usage)::bigint AS request_count,
           COUNT(*) FILTER (WHERE NOT b.valid_duration)::bigint AS invalid_duration_count,
           COUNT(*) FILTER (WHERE b.valid_duration AND NOT b.successful)::bigint AS error_count,
           COUNT(*) FILTER (WHERE b.valid_duration AND b.successful AND NOT b.has_usage)::bigint AS missing_usage_count,
           COALESCE(SUM(b.fresh) FILTER (WHERE b.valid_duration AND b.successful AND b.has_usage), 0)::bigint AS fresh_tokens,
           COALESCE(SUM(b.cache_read) FILTER (WHERE b.valid_duration AND b.successful AND b.has_usage), 0)::bigint AS cache_read_tokens,
           COALESCE(SUM(b.cache_write) FILTER (WHERE b.valid_duration AND b.successful AND b.has_usage), 0)::bigint AS cache_write_tokens,
           COALESCE(SUM(b.output) FILTER (WHERE b.valid_duration AND b.successful AND b.has_usage), 0)::bigint AS output_tokens
    FROM intervals i JOIN buckets b ON b.attributed_at > i.start_at AND b.attributed_at <= i.end_at
    GROUP BY i.interval_index, b.bucket, b.bucket_model
)
SELECT interval_index AS "interval_index!", bucket AS "bucket!", model,
       request_count AS "request_count!", invalid_duration_count AS "invalid_duration_count!",
       error_count AS "error_count!", missing_usage_count AS "missing_usage_count!",
       fresh_tokens AS "fresh_tokens!", cache_read_tokens AS "cache_read_tokens!",
       cache_write_tokens AS "cache_write_tokens!", output_tokens AS "output_tokens!",
       (SELECT COUNT(*) FROM ranked)::bigint AS "distinct_models!"
FROM grouped ORDER BY interval_index, bucket, model
