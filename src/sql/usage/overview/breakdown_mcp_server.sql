WITH normalized AS (
    SELECT COALESCE(rr.mcp_server_name, ms.name, '(unknown)') AS label,
           rr.mcp_server_id,
           ms.provider_kind AS server_provider_kind,
           rr.ok,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens,
           -- Post-0072 the stored `input_tokens` is already the ordinary
           -- (non-cache) value, so use it directly; the cache must not be
           -- subtracted again (double-subtract of backfilled rows).
           -- Closed loop: ordinary + cache_read + cache_write + output == total.
           GREATEST(COALESCE(rr.input_tokens, 0), 0)::BIGINT AS normalized_input_tokens,
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
    LEFT JOIN mcp_servers ms ON ms.server_id = rr.mcp_server_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
), grouped AS (
    SELECT label,
           mcp_server_id,
           server_provider_kind,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS success_count,
           COUNT(*) FILTER (
               WHERE normalized_cache_read_tokens > 0
           )::BIGINT AS cache_hit_count,
           COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           -- Issue #342: persisted closed-loop total summed once per row, so
           -- the grouped MCP exit reconciles with `usage_summary`.
           COALESCE(SUM(GREATEST(total_tokens, 0)), 0)::BIGINT AS total_tokens,
           -- Per-row-summed fold-aware denominator for `cache_rate`; still-folded
           -- rows use the `max(input, read+write)` fallback, so the aggregate must
           -- not re-derive the guard on the raw input SUM (issue #338).
           COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS full_input_tokens
    FROM normalized
    GROUP BY label, mcp_server_id, server_provider_kind
), totals AS (
    SELECT SUM(request_count)::DOUBLE PRECISION AS request_count
    FROM grouped
)
SELECT label AS "label!",
       NULL::TEXT AS model,
       mcp_server_id,
       grouped.server_provider_kind AS "server_provider_kind",
       grouped.request_count AS "request_count!",
       COALESCE(grouped.request_count::DOUBLE PRECISION / NULLIF(totals.request_count, 0), 0) AS "request_share!",
       success_count AS "success_count!",
       NULL::DOUBLE PRECISION AS token_share,
       cache_hit_count AS "cache_hit_count!",
       input_tokens AS "input_tokens!",
       cache_read_tokens AS "cache_read_tokens!",
       cache_write_tokens AS "cache_write_tokens!",
       output_tokens AS "output_tokens!",
       grouped.total_tokens AS "total_tokens!",
       grouped.full_input_tokens AS "full_input_tokens!",
       NULL::DOUBLE PRECISION AS avg_output_tokens_per_second
FROM grouped
CROSS JOIN totals
ORDER BY grouped.request_count DESC, grouped.label ASC
LIMIT 50
