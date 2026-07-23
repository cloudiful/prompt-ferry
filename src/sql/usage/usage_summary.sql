SELECT
    COUNT(*)::BIGINT AS "request_count!",
    COUNT(*) FILTER (WHERE request_state = 'completed')::BIGINT AS "success_count!",
    COUNT(*) FILTER (WHERE request_state IN ('failed', 'aborted'))::BIGINT AS "error_count!",
    COALESCE(SUM(input_tokens), 0)::BIGINT AS "input_tokens!",
    COALESCE(SUM(output_tokens), 0)::BIGINT AS "output_tokens!",
    COALESCE(SUM(total_tokens), 0)::BIGINT AS "total_tokens!",
    COALESCE(SUM(cached_tokens), 0)::BIGINT AS "cached_tokens!",
    CASE WHEN COALESCE(SUM(input_tokens), 0) > 0
        THEN COALESCE(SUM(cached_tokens), 0)::DOUBLE PRECISION / SUM(input_tokens)::DOUBLE PRECISION
        ELSE NULL
    END AS cache_rate,
    AVG(duration_ms)::DOUBLE PRECISION AS avg_duration_ms
FROM request_records
WHERE event_kind = 'request'
  AND created_at >= NOW() - ($1::BIGINT * INTERVAL '1 day')
  AND ($2::BIGINT IS NULL OR user_id = $2)
