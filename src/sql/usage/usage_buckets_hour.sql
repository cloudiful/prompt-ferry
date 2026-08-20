WITH bounds AS (
    SELECT date_trunc('hour', COALESCE($2::TIMESTAMPTZ, NOW() - ($1::BIGINT * INTERVAL '1 hour'))) AS start_at,
           date_trunc('hour', COALESCE($3::TIMESTAMPTZ, NOW())) AS end_at
),
buckets AS (
    SELECT generate_series(start_at, end_at, INTERVAL '1 hour') AS bucket_at FROM bounds
),
normalized AS (
    SELECT date_trunc('hour', rr.created_at) AS bucket_at,
           rr.request_state,
           rr.duration_ms,
           rr.ttft_ms,
           GREATEST(COALESCE(rr.input_tokens, 0), 0)::BIGINT AS input_tokens,
           GREATEST(COALESCE(rr.output_tokens, 0), 0)::BIGINT AS output_tokens,
           GREATEST(COALESCE(rr.total_tokens, 0), 0)::BIGINT AS total_tokens,
           GREATEST(
               COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0),
               0
           )::BIGINT AS normalized_cache_read_tokens,
           GREATEST(COALESCE(rr.cached_tokens, 0), 0)::BIGINT AS normalized_cached_tokens,
           GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)::BIGINT AS normalized_cache_write_tokens,
           GREATEST(
               COALESCE(rr.input_tokens, 0),
               GREATEST(COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0), 0)
                   + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
               0
           )::BIGINT AS normalized_full_input_tokens
    FROM request_records rr, bounds
    WHERE rr.event_kind = 'request'
      AND rr.created_at >= bounds.start_at AND rr.created_at < bounds.end_at + INTERVAL '1 hour'
      AND ($4::BIGINT IS NULL OR rr.user_id = $4)
      AND ($5::TEXT IS NULL OR rr.request_category = $5::TEXT)
),
agg AS (
    SELECT bucket_at,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE request_state IN ('failed', 'aborted'))::BIGINT AS error_count,
           COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(normalized_cached_tokens), 0)::BIGINT AS cached_tokens,
           COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS cache_read_total,
           COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS full_input_total,
           AVG(duration_ms)::DOUBLE PRECISION AS avg_duration_ms,
           AVG(ttft_ms)::DOUBLE PRECISION AS avg_ttft_ms
    FROM normalized
    GROUP BY bucket_at
)
SELECT buckets.bucket_at AS "bucket_at!",
       COALESCE(agg.request_count, 0)::BIGINT AS "request_count!",
       COALESCE(agg.error_count, 0)::BIGINT AS "error_count!",
       COALESCE(agg.input_tokens, 0)::BIGINT AS "input_tokens!",
       COALESCE(agg.output_tokens, 0)::BIGINT AS "output_tokens!",
       COALESCE(agg.total_tokens, 0)::BIGINT AS "total_tokens!",
       COALESCE(agg.cached_tokens, 0)::BIGINT AS "cached_tokens!",
       CASE WHEN COALESCE(agg.full_input_total, 0) > 0
            THEN LEAST(
                1.0::DOUBLE PRECISION,
                COALESCE(agg.cache_read_total, 0)::DOUBLE PRECISION
                    / agg.full_input_total::DOUBLE PRECISION
            )
            ELSE NULL
       END AS cache_rate,
       CASE WHEN COALESCE(agg.request_count, 0) > 0
            THEN COALESCE(agg.error_count, 0)::DOUBLE PRECISION / agg.request_count::DOUBLE PRECISION
            ELSE NULL
       END AS error_rate,
       agg.avg_duration_ms AS avg_duration_ms,
       agg.avg_ttft_ms AS avg_ttft_ms
FROM buckets
LEFT JOIN agg USING (bucket_at)
ORDER BY buckets.bucket_at ASC