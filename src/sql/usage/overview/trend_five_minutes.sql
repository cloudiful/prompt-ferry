WITH rows AS (
    SELECT usage.*
    FROM public.request_usage_window($1::BIGINT, $2::TEXT, $3::TIMESTAMPTZ, $4::TIMESTAMPTZ, $6::TEXT) usage
    WHERE ($5::TEXT IS NULL OR COALESCE(
        (SELECT u.login_name FROM users u WHERE u.user_id = usage.user_id),
        CASE WHEN usage.user_id IS NULL THEN '-' ELSE '#' || usage.user_id::TEXT END
    ) = $5)
), aggregates AS (
    SELECT date_bin(interval '5 minutes', bucket_at, '1970-01-01 00:00:00+00'::TIMESTAMPTZ) AS bucket_at,
           SUM(request_count)::BIGINT AS request_count,
           SUM(success_by_ok)::BIGINT AS success_count,
           SUM(error_by_ok)::BIGINT AS error_count,
           SUM(cache_hit_count)::BIGINT AS cache_hit_count,
           SUM(input_tokens)::BIGINT AS input_tokens,
           SUM(cache_read_tokens)::BIGINT AS cache_read_tokens,
           SUM(cache_write_tokens)::BIGINT AS cache_write_tokens,
           SUM(output_tokens)::BIGINT AS output_tokens,
           SUM(total_tokens)::BIGINT AS total_tokens,
           SUM(full_input_tokens)::BIGINT AS full_input_tokens
    FROM rows
    GROUP BY 1
), p95 AS (
    SELECT date_bin(interval '5 minutes', rr.created_at, '1970-01-01 00:00:00+00'::TIMESTAMPTZ) AS bucket_at,
           percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms)
               FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms,
           percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.ttft_ms)
               FILTER (WHERE rr.ttft_ms IS NOT NULL) AS p95_first_token_ms
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND rr.created_at >= $3::TIMESTAMPTZ AND rr.created_at < $4::TIMESTAMPTZ
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
    GROUP BY 1
), buckets AS (
    SELECT generate_series(
        date_bin(interval '5 minutes', $3::TIMESTAMPTZ, '1970-01-01 00:00:00+00'::TIMESTAMPTZ),
        date_bin(interval '5 minutes', $4::TIMESTAMPTZ - interval '1 microsecond', '1970-01-01 00:00:00+00'::TIMESTAMPTZ),
        interval '5 minutes'
    ) AS bucket_at
    WHERE $3::TIMESTAMPTZ < $4::TIMESTAMPTZ AND $4::TIMESTAMPTZ - $3::TIMESTAMPTZ <= interval '5 hours'
)
SELECT b.bucket_at AS "bucket_at!", COALESCE(a.request_count, 0)::BIGINT AS "request_count!",
       COALESCE(a.success_count, 0)::BIGINT AS "success_count!", COALESCE(a.error_count, 0)::BIGINT AS "error_count!",
       COALESCE(a.cache_hit_count, 0)::BIGINT AS "cache_hit_count!", COALESCE(a.input_tokens, 0)::BIGINT AS "input_tokens!",
       COALESCE(a.cache_read_tokens, 0)::BIGINT AS "cache_read_tokens!", COALESCE(a.cache_write_tokens, 0)::BIGINT AS "cache_write_tokens!",
       COALESCE(a.output_tokens, 0)::BIGINT AS "output_tokens!", COALESCE(a.total_tokens, 0)::BIGINT AS "total_tokens!",
       COALESCE(a.full_input_tokens, 0)::BIGINT AS "full_input_tokens!", p.p95_total_ms, p.p95_first_token_ms
FROM buckets b
LEFT JOIN aggregates a ON a.bucket_at = b.bucket_at
LEFT JOIN p95 p ON p.bucket_at = b.bucket_at
ORDER BY b.bucket_at ASC
