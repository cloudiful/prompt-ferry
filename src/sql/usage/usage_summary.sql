WITH windows AS (
    SELECT NOW() - ($1::BIGINT * INTERVAL '1 day') AS start_at
),
rows AS (
    SELECT usage.*
    FROM windows
    CROSS JOIN LATERAL public.request_usage_window(
        $2::BIGINT,
        NULL,
        windows.start_at,
        NULL,
        'all'
    ) usage
)
SELECT
    COALESCE(SUM(request_count), 0)::BIGINT AS "request_count!",
    COALESCE(SUM(success_by_state), 0)::BIGINT AS "success_count!",
    COALESCE(SUM(error_by_state), 0)::BIGINT AS "error_count!",
    COALESCE(SUM(input_tokens_raw), 0)::BIGINT AS "input_tokens!",
    COALESCE(SUM(output_tokens_clamped), 0)::BIGINT AS "output_tokens!",
    COALESCE(SUM(total_tokens), 0)::BIGINT AS "total_tokens!",
    COALESCE(SUM(cached_tokens), 0)::BIGINT AS "cached_tokens!",
    CASE WHEN COALESCE(SUM(full_input_tokens), 0) > 0
        THEN LEAST(
            1.0::DOUBLE PRECISION,
            COALESCE(SUM(cache_read_tokens_clamped), 0)::DOUBLE PRECISION
                / SUM(full_input_tokens)::DOUBLE PRECISION
        )
        ELSE NULL
    END AS cache_rate,
    CASE WHEN COALESCE(SUM(duration_count), 0) > 0
        THEN SUM(duration_sum_ms)::DOUBLE PRECISION / SUM(duration_count)::DOUBLE PRECISION
        ELSE NULL
    END AS avg_duration_ms
FROM rows
