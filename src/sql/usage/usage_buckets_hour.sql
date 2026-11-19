WITH bounds AS (
    SELECT date_trunc('hour', COALESCE($2::TIMESTAMPTZ, NOW() - ($1::BIGINT * INTERVAL '1 hour'))) AS start_at,
           date_trunc('hour', COALESCE($3::TIMESTAMPTZ, NOW())) AS end_at
),
buckets AS (
    SELECT generate_series(start_at, end_at, INTERVAL '1 hour') AS bucket_at FROM bounds
),
rows AS (
    SELECT usage.*
    FROM bounds
    CROSS JOIN LATERAL public.request_usage_window(
        $4::BIGINT,
        $5::TEXT,
        bounds.start_at,
        bounds.end_at + INTERVAL '1 hour',
        'all'
    ) usage
),
agg AS (
    SELECT date_trunc('hour', bucket_at) AS bucket_at,
           SUM(request_count)::BIGINT AS request_count,
           SUM(error_by_state)::BIGINT AS error_count,
           SUM(input_tokens_raw)::BIGINT AS input_tokens,
           SUM(output_tokens_clamped)::BIGINT AS output_tokens,
           SUM(total_tokens)::BIGINT AS total_tokens,
           SUM(cached_tokens)::BIGINT AS cached_tokens,
           SUM(cache_read_tokens_clamped)::BIGINT AS cache_read_total,
           SUM(full_input_tokens)::BIGINT AS full_input_total,
           SUM(duration_sum_ms)::DOUBLE PRECISION / NULLIF(SUM(duration_count), 0) AS avg_duration_ms,
           SUM(ttft_sum_ms)::DOUBLE PRECISION / NULLIF(SUM(ttft_count), 0) AS avg_ttft_ms
    FROM rows
    GROUP BY 1
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
