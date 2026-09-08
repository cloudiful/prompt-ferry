WITH normalized AS (
    SELECT rr.*,
           -- Post-0072 the stored `input_tokens` is already the ordinary
           -- (non-cache) value, so use it directly; the cache must not be
           -- subtracted again (double-subtract of backfilled rows).
           -- Closed loop: ordinary + cache_read + cache_write + output == total.
           GREATEST(COALESCE(rr.input_tokens, 0), 0)::BIGINT AS normalized_input_tokens,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens,
            -- P2 (issue #205): full-input denominator `ordinary+read+write`,
            -- 真 0.49 (e.g. 9728/18144) vs old `max` 1.0 which dropped the
            -- ordinary part. Still-folded rows (0072/0073 guard:
            -- cache>0 AND total>=output AND input>=total-output, input already
            -- holds the cache) fall back to `max(input, read+write)` to avoid
            -- the ≈1.9x double-count; total keeps closed-loop
            -- ordinary+read+write+output.
            CASE
                WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                    AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                    AND COALESCE(rr.input_tokens, 0)
                        >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
                THEN GREATEST(
                    COALESCE(rr.input_tokens, 0),
                    GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                        + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                    0
                )
                ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
                    + GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)
            END::BIGINT AS normalized_full_input_tokens
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
        -- P1 (issue #226): the aggregated `cache_rate` denominator must be
        -- SUM(normalized_full_input_tokens) — computed row-by-row in the CTE
        -- so still-folded rows use the `max` fallback — not the raw
        -- `input_tokens` SUM, which double-counts the cache (49% vs 98.58%).
        COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS "full_input_tokens!",
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
