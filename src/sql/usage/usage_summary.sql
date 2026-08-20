WITH normalized AS (
    SELECT
        request_state,
        duration_ms,
        GREATEST(COALESCE(input_tokens, 0), 0)::BIGINT AS input_tokens,
        GREATEST(COALESCE(output_tokens, 0), 0)::BIGINT AS output_tokens,
        GREATEST(COALESCE(total_tokens, 0), 0)::BIGINT AS total_tokens,
        GREATEST(
            COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0),
            0
        )::BIGINT AS normalized_cache_read_tokens,
        GREATEST(COALESCE(cached_tokens, 0), 0)::BIGINT AS normalized_cached_tokens,
        GREATEST(COALESCE(cache_write_tokens, 0), 0)::BIGINT AS normalized_cache_write_tokens,
        GREATEST(
            COALESCE(input_tokens, 0),
            GREATEST(COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0), 0)
                + GREATEST(COALESCE(cache_write_tokens, 0), 0),
            0
        )::BIGINT AS normalized_full_input_tokens
    FROM request_records
    WHERE event_kind = 'request'
      AND created_at >= NOW() - ($1::BIGINT * INTERVAL '1 day')
      AND ($2::BIGINT IS NULL OR user_id = $2)
)
SELECT
    COUNT(*)::BIGINT AS "request_count!",
    COUNT(*) FILTER (WHERE request_state = 'completed')::BIGINT AS "success_count!",
    COUNT(*) FILTER (WHERE request_state IN ('failed', 'aborted'))::BIGINT AS "error_count!",
    COALESCE(SUM(input_tokens), 0)::BIGINT AS "input_tokens!",
    COALESCE(SUM(output_tokens), 0)::BIGINT AS "output_tokens!",
    COALESCE(SUM(total_tokens), 0)::BIGINT AS "total_tokens!",
    COALESCE(SUM(normalized_cached_tokens), 0)::BIGINT AS "cached_tokens!",
    CASE WHEN COALESCE(SUM(normalized_full_input_tokens), 0) > 0
        THEN LEAST(
            1.0::DOUBLE PRECISION,
            COALESCE(SUM(normalized_cache_read_tokens), 0)::DOUBLE PRECISION
                / SUM(normalized_full_input_tokens)::DOUBLE PRECISION
        )
        ELSE NULL
    END AS cache_rate,
    AVG(duration_ms)::DOUBLE PRECISION AS avg_duration_ms
FROM normalized