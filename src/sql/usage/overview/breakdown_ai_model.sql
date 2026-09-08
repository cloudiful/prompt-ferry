WITH normalized AS (
    SELECT rr.model,
           rr.ok,
           rr.request_state,
           rr.output_tokens AS raw_output_tokens,
           rr.duration_ms,
           -- Post-0072 the stored `input_tokens` is already the ordinary
           -- (non-cache) value, so use it directly; the cache must not be
           -- subtracted again (double-subtract of backfilled rows).
           -- Closed loop: ordinary + cache_read + cache_write + output == total.
           GREATEST(COALESCE(rr.input_tokens, 0), 0)::BIGINT AS normalized_input_tokens,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens,
           -- P2 (issue #200): full-input denominator `max(input, read+write)`,
           -- 仿 usage_buckets_day:33-44 + usage_events_page LEAST, 仅供分母参考
           -- （presentation用max）；total保持基线闭环
           -- ordinary+read+write+output，不用max以免少min(ordinary,cache)。
           -- A still-folded input already contains the cache, so the presentation
           -- `max` denominator avoids the ≈1.9x double-count (47-50%).
           GREATEST(
               COALESCE(rr.input_tokens, 0),
               GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                   + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
               0
           )::BIGINT AS normalized_full_input_tokens,
           COALESCE(rr.output_tokens, 0)::BIGINT AS output_tokens
    FROM request_records rr
    LEFT JOIN users u ON u.user_id = rr.user_id
    WHERE rr.event_kind = 'request'
      AND rr.request_category = $2
      AND ($1::BIGINT IS NULL OR rr.user_id = $1)
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4)
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5)
), grouped AS (
    SELECT COALESCE(model, '(unknown)') AS label,
           model,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS success_count,
            COUNT(*) FILTER (
                WHERE normalized_cache_read_tokens > 0
            )::BIGINT AS cache_hit_count,
           COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(
               normalized_input_tokens
                   + normalized_cache_read_tokens
                   + normalized_cache_write_tokens
                   + output_tokens
           ), 0)::BIGINT AS total_tokens,
           AVG(
               CASE
                   WHEN request_state = 'completed'
                       AND raw_output_tokens IS NOT NULL
                       AND raw_output_tokens > 0
                       AND duration_ms IS NOT NULL
                       AND duration_ms > 0
                   THEN raw_output_tokens::NUMERIC / duration_ms::NUMERIC * 1000.0
                   ELSE NULL
               END
           )::DOUBLE PRECISION AS avg_output_tokens_per_second
    FROM normalized
    GROUP BY model
), totals AS (
    SELECT SUM(request_count)::DOUBLE PRECISION AS request_count,
           SUM(total_tokens)::DOUBLE PRECISION AS total_tokens
    FROM grouped
)
SELECT label AS "label!",
       model,
       NULL::UUID AS mcp_server_id,
       grouped.request_count AS "request_count!",
       COALESCE(grouped.request_count::DOUBLE PRECISION / NULLIF(totals.request_count, 0), 0) AS "request_share!",
       success_count AS "success_count!",
       grouped.total_tokens::DOUBLE PRECISION / NULLIF(totals.total_tokens, 0) AS token_share,
       cache_hit_count AS "cache_hit_count!",
       input_tokens AS "input_tokens!",
       cache_read_tokens AS "cache_read_tokens!",
       cache_write_tokens AS "cache_write_tokens!",
       output_tokens AS "output_tokens!",
       grouped.total_tokens AS "total_tokens!",
       grouped.avg_output_tokens_per_second AS avg_output_tokens_per_second
FROM grouped
CROSS JOIN totals
ORDER BY grouped.total_tokens DESC, grouped.request_count DESC, grouped.label ASC
LIMIT 50
