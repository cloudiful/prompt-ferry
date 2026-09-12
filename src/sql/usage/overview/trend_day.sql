WITH normalized AS (
    SELECT date_trunc('day', rr.created_at) AS bucket_at,
           rr.ok,
           rr.request_state,
           rr.duration_ms,
           rr.ttft_ms,
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
            END::BIGINT AS normalized_full_input_tokens,
           COALESCE(rr.output_tokens, 0)::BIGINT AS output_tokens,
           COALESCE(rr.total_tokens, 0)::BIGINT AS total_tokens
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
)
SELECT bucket_at AS "bucket_at!",
       COUNT(*)::BIGINT AS "request_count!",
       COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS "success_count!",
       COUNT(*) FILTER (
           WHERE ok IS FALSE OR request_state IN ('failed', 'aborted')
       )::BIGINT AS "error_count!",
       COUNT(*) FILTER (
           WHERE normalized_cache_read_tokens > 0
       )::BIGINT AS "cache_hit_count!",
       COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS "input_tokens!",
       COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS "cache_read_tokens!",
       COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS "cache_write_tokens!",
       COALESCE(SUM(output_tokens), 0)::BIGINT AS "output_tokens!",
       -- Issue #342: use the persisted closed-loop total so each trend bucket
       -- reconciles with `usage_summary.sql`/bucket queries instead of
       -- re-adding the cache meters for still-folded rows.
       COALESCE(SUM(GREATEST(total_tokens, 0)), 0)::BIGINT AS "total_tokens!",
       -- Per-row-summed fold-aware denominator for `cache_rate`; still-folded
       -- rows use the `max(input, read+write)` fallback, so the aggregate must
       -- not re-derive the guard on the raw input SUM (issue #338).
       COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS "full_input_tokens!",
       percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)
           FILTER (WHERE duration_ms IS NOT NULL) AS p95_total_ms,
       percentile_cont(0.95) WITHIN GROUP (ORDER BY ttft_ms)
           FILTER (WHERE ttft_ms IS NOT NULL) AS p95_first_token_ms
FROM normalized
GROUP BY bucket_at
ORDER BY bucket_at ASC
