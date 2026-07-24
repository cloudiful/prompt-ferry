WITH bounds AS (
    SELECT date_trunc('hour', COALESCE($2::TIMESTAMPTZ, NOW() - ($1::BIGINT * INTERVAL '1 hour'))) AS start_at,
           date_trunc('hour', COALESCE($3::TIMESTAMPTZ, NOW())) AS end_at
),
buckets AS (
    SELECT generate_series(start_at, end_at, INTERVAL '1 hour') AS bucket_at FROM bounds
),
agg AS (
    SELECT date_trunc('hour', created_at) AS bucket_at,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE request_state IN ('failed', 'aborted'))::BIGINT AS error_count,
           COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(cached_tokens), 0)::BIGINT AS cached_tokens,
           AVG(duration_ms)::DOUBLE PRECISION AS avg_duration_ms,
           AVG(ttft_ms)::DOUBLE PRECISION AS avg_ttft_ms
    FROM request_records, bounds
    WHERE event_kind = 'request'
      AND created_at >= bounds.start_at AND created_at < bounds.end_at + INTERVAL '1 hour'
      AND ($4::BIGINT IS NULL OR user_id = $4)
      AND ($5::TEXT IS NULL OR request_category = $5::TEXT)
    GROUP BY 1
)
SELECT buckets.bucket_at AS "bucket_at!",
       COALESCE(agg.request_count, 0)::BIGINT AS "request_count!",
       COALESCE(agg.error_count, 0)::BIGINT AS "error_count!",
       COALESCE(agg.input_tokens, 0)::BIGINT AS "input_tokens!",
       COALESCE(agg.output_tokens, 0)::BIGINT AS "output_tokens!",
       COALESCE(agg.total_tokens, 0)::BIGINT AS "total_tokens!",
       COALESCE(agg.cached_tokens, 0)::BIGINT AS "cached_tokens!",
       CASE WHEN COALESCE(agg.input_tokens, 0) > 0
            THEN COALESCE(agg.cached_tokens, 0)::DOUBLE PRECISION / agg.input_tokens::DOUBLE PRECISION
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
