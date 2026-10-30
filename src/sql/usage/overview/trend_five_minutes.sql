WITH normalized AS (
    SELECT date_bin(interval '5 minutes', rr.created_at, '1970-01-01 00:00:00+00'::TIMESTAMPTZ) AS bucket_at,
           rr.ok, rr.request_state, rr.duration_ms, rr.ttft_ms,
           CASE WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                    AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                    AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
                THEN GREATEST(COALESCE(rr.input_tokens, 0) - GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0) - GREATEST(COALESCE(rr.cache_write_tokens, 0), 0), 0)
                ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0) END::BIGINT AS normalized_input_tokens,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens,
           CASE WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                    AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                    AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
                THEN GREATEST(COALESCE(rr.input_tokens, 0), GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0) + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0), 0)
                ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0) + GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0) + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0) END::BIGINT AS normalized_full_input_tokens,
           COALESCE(rr.output_tokens, 0)::BIGINT AS output_tokens,
           COALESCE(rr.total_tokens, 0)::BIGINT AS total_tokens
    FROM request_records rr LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request' AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND rr.created_at >= $3::TIMESTAMPTZ AND rr.created_at < $4::TIMESTAMPTZ
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
      AND ($6::TEXT = 'all' OR $2 <> 'mcp'
        OR ($6::TEXT = 'service' AND (rr.mcp_server_name IS NOT NULL OR rr.mcp_server_id IS NOT NULL))
        OR ($6::TEXT = 'gateway' AND rr.mcp_server_name IS NULL AND rr.mcp_server_id IS NULL))
), aggregates AS (
    SELECT bucket_at, COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS success_count,
           COUNT(*) FILTER (WHERE ok IS FALSE OR request_state IN ('failed', 'aborted'))::BIGINT AS error_count,
           COUNT(*) FILTER (WHERE normalized_cache_read_tokens > 0)::BIGINT AS cache_hit_count,
           COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(GREATEST(total_tokens, 0)), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS full_input_tokens,
           percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) FILTER (WHERE duration_ms IS NOT NULL) AS p95_total_ms,
           percentile_cont(0.95) WITHIN GROUP (ORDER BY ttft_ms) FILTER (WHERE ttft_ms IS NOT NULL) AS p95_first_token_ms
    FROM normalized GROUP BY bucket_at
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
       COALESCE(a.full_input_tokens, 0)::BIGINT AS "full_input_tokens!", a.p95_total_ms, a.p95_first_token_ms
FROM buckets b LEFT JOIN aggregates a ON a.bucket_at = b.bucket_at ORDER BY b.bucket_at ASC
