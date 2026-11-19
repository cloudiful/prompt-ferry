WITH rows AS (
    SELECT usage.*
    FROM public.request_usage_window($1::BIGINT, $2::TEXT, $3::TIMESTAMPTZ, $4::TIMESTAMPTZ, $6::TEXT) usage
    WHERE ($5::TEXT IS NULL OR COALESCE(
        (SELECT u.login_name FROM users u WHERE u.user_id = usage.user_id),
        CASE WHEN usage.user_id IS NULL THEN '-' ELSE '#' || usage.user_id::TEXT END
    ) = $5)
), p95 AS (
    SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms)
               FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms,
           percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.ttft_ms)
               FILTER (WHERE rr.ttft_ms IS NOT NULL) AS p95_first_token_ms
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
      AND (
          $6::TEXT = 'all'
          OR $2 <> 'mcp'
          OR (
              $6::TEXT = 'service'
              AND (rr.mcp_server_name IS NOT NULL OR rr.mcp_server_id IS NOT NULL)
          )
          OR (
              $6::TEXT = 'gateway'
              AND rr.mcp_server_name IS NULL
              AND rr.mcp_server_id IS NULL
          )
      )
), agg AS (
    SELECT COALESCE(SUM(request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(success_by_ok), 0)::BIGINT AS success_count,
           COALESCE(SUM(error_by_ok), 0)::BIGINT AS error_count,
           COALESCE(SUM(cache_hit_count), 0)::BIGINT AS cache_hit_count,
           COUNT(DISTINCT mcp_protocol_method)
               FILTER (WHERE mcp_protocol_method IS NOT NULL)::BIGINT AS method_count,
           COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(full_input_tokens), 0)::BIGINT AS full_input_tokens,
           SUM(tps_sum)::DOUBLE PRECISION / NULLIF(SUM(tps_count), 0) AS avg_output_tokens_per_second
    FROM rows
)
SELECT agg.request_count::BIGINT AS "request_count!",
       agg.success_count::BIGINT AS "success_count!",
       agg.error_count::BIGINT AS "error_count!",
       agg.cache_hit_count::BIGINT AS "cache_hit_count!",
       agg.method_count::BIGINT AS "method_count!",
       agg.input_tokens::BIGINT AS "input_tokens!",
       agg.cache_read_tokens::BIGINT AS "cache_read_tokens!",
       agg.cache_write_tokens::BIGINT AS "cache_write_tokens!",
       agg.output_tokens::BIGINT AS "output_tokens!",
       agg.total_tokens::BIGINT AS "total_tokens!",
       agg.full_input_tokens::BIGINT AS "full_input_tokens!",
       agg.avg_output_tokens_per_second,
       p95.p95_total_ms,
       p95.p95_first_token_ms
FROM agg
CROSS JOIN p95
