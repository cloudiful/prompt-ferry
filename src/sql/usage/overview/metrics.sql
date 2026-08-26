WITH normalized AS (
    SELECT rr.*,
           GREATEST(
               COALESCE(rr.input_tokens, 0)
                   - COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)
                   - COALESCE(rr.cache_write_tokens, 0),
               0
           )::BIGINT AS normalized_input_tokens,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
)
SELECT COUNT(*)::BIGINT AS "request_count!",
       COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS "success_count!",
       COUNT(*) FILTER (
           WHERE ok IS FALSE OR request_state IN ('failed', 'aborted')
       )::BIGINT AS "error_count!",
       COUNT(*) FILTER (
           WHERE normalized_cache_read_tokens > 0
       )::BIGINT AS "cache_hit_count!",
       COUNT(DISTINCT mcp_protocol_method)
           FILTER (WHERE mcp_protocol_method IS NOT NULL)::BIGINT AS "method_count!",
       COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS "input_tokens!",
       COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS "cache_read_tokens!",
       COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS "cache_write_tokens!",
       COALESCE(SUM(output_tokens), 0)::BIGINT AS "output_tokens!",
       COALESCE(SUM(
           normalized_input_tokens
               + normalized_cache_read_tokens
               + normalized_cache_write_tokens
               + COALESCE(output_tokens, 0)
       ), 0)::BIGINT AS "total_tokens!",
       AVG(
           CASE
               WHEN request_category = 'ai'
                   AND request_state = 'completed'
                   AND output_tokens IS NOT NULL
                   AND output_tokens > 0
                   AND duration_ms > 0
               THEN output_tokens::NUMERIC / duration_ms::NUMERIC * 1000.0
               ELSE NULL
           END
       )::DOUBLE PRECISION AS avg_output_tokens_per_second,
       percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)
           FILTER (WHERE duration_ms IS NOT NULL) AS p95_total_ms,
       percentile_cont(0.95) WITHIN GROUP (ORDER BY ttft_ms)
           FILTER (WHERE ttft_ms IS NOT NULL) AS p95_first_token_ms
FROM normalized
