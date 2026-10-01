WITH normalized AS (
    -- Upstream perspective aggregates the raw request rows (no model-level
    -- LIMIT 50) so endpoint totals and per-model detail both use the full
    -- filtered window. `effective_model` is the route-target override when set,
    -- otherwise the originally requested model.
    SELECT COALESCE(rr.upstream_model, rr.model, '(unknown)') AS effective_model,
           rr.ok,
           rr.request_state,
           rr.endpoint_id,
           rr.output_tokens AS raw_output_tokens,
           rr.duration_ms,
           -- Post-0072 rows already store the ordinary (non-cache) input; rows
           -- still in the folded shape keep the cache inside `input_tokens`, so
           -- expand them to the ordinary miss `input - cache_read - cache_write`
           -- (floored at 0) using the same guard as the full-input denominator.
           -- Closed loop: ordinary + cache_read + cache_write + output == total.
           CASE
               WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                   AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                   AND COALESCE(rr.input_tokens, 0)
                       >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
               THEN GREATEST(
                   COALESCE(rr.input_tokens, 0)
                       - GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                       - GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                   0
               )
               ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
           END::BIGINT AS normalized_input_tokens,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           COALESCE(rr.cache_write_tokens, 0)::BIGINT AS normalized_cache_write_tokens,
           -- P2 (issue #205): full-input denominator `ordinary+read+write`.
           -- Still-folded rows (0072/0073 guard) fall back to `max(input,
           -- read+write)` to avoid the ≈1.9x double-count.
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
), grouped AS (
    -- Endpoint identity is `endpoint_id`; requests without one collapse into
    -- the `(direct)` row instead of merging by a shared name.
    SELECT COALESCE(pe.name, n.endpoint_id::TEXT, '(direct)') AS label,
           n.endpoint_id,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE n.ok IS TRUE)::BIGINT AS success_count,
           COUNT(*) FILTER (
               WHERE n.ok IS FALSE OR n.request_state IN ('failed', 'aborted')
           )::BIGINT AS error_count,
           COUNT(*) FILTER (
               WHERE n.normalized_cache_read_tokens > 0
           )::BIGINT AS cache_hit_count,
           COALESCE(SUM(n.normalized_input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(n.normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.normalized_cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(n.output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(GREATEST(n.total_tokens, 0)), 0)::BIGINT AS total_tokens,
           -- Issue #226: aggregated cache denominator is the row-by-row
           -- fold-aware SUM, not the raw `input_tokens` SUM.
           COALESCE(SUM(n.normalized_full_input_tokens), 0)::BIGINT AS full_input_tokens,
           AVG(
               CASE
                   WHEN n.request_state = 'completed'
                       AND n.raw_output_tokens IS NOT NULL
                       AND n.raw_output_tokens > 0
                       AND n.duration_ms IS NOT NULL
                       AND n.duration_ms > 0
                   THEN n.raw_output_tokens::NUMERIC / n.duration_ms::NUMERIC * 1000.0
                   ELSE NULL
               END
           )::DOUBLE PRECISION AS avg_output_tokens_per_second
    FROM normalized n
    LEFT JOIN provider_endpoints pe ON pe.endpoint_id = n.endpoint_id
    GROUP BY COALESCE(pe.name, n.endpoint_id::TEXT, '(direct)'), n.endpoint_id
), totals AS (
    SELECT SUM(request_count)::DOUBLE PRECISION AS request_count,
           SUM(total_tokens)::DOUBLE PRECISION AS total_tokens
    FROM grouped
), models AS (
    SELECT n.endpoint_id,
           n.effective_model,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (
               WHERE n.ok IS FALSE OR n.request_state IN ('failed', 'aborted')
           )::BIGINT AS error_count,
           COALESCE(SUM(GREATEST(n.total_tokens, 0)), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(n.normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.normalized_full_input_tokens), 0)::BIGINT AS full_input_tokens,
           AVG(
               CASE
                   WHEN n.request_state = 'completed'
                       AND n.raw_output_tokens IS NOT NULL
                       AND n.raw_output_tokens > 0
                       AND n.duration_ms IS NOT NULL
                       AND n.duration_ms > 0
                   THEN n.raw_output_tokens::NUMERIC / n.duration_ms::NUMERIC * 1000.0
                   ELSE NULL
               END
           )::DOUBLE PRECISION AS avg_output_tokens_per_second
    FROM normalized n
    GROUP BY n.endpoint_id, n.effective_model
), model_agg AS (
    SELECT m.endpoint_id,
           json_agg(
               json_build_object(
                   'model', m.effective_model,
                   'request_count', m.request_count,
                   'request_share', CASE WHEN t.request_count > 0 THEN m.request_count::DOUBLE PRECISION / t.request_count ELSE 0 END,
                   'error_count', m.error_count,
                   'error_rate', CASE WHEN m.request_count > 0 THEN m.error_count::DOUBLE PRECISION / m.request_count ELSE 0 END,
                   'total_tokens', m.total_tokens,
                   'token_share', m.total_tokens::DOUBLE PRECISION / NULLIF(t.total_tokens, 0),
                   'cache_rate', CASE
                       WHEN m.full_input_tokens > 0
                       THEN LEAST(GREATEST(m.cache_read_tokens::DOUBLE PRECISION / m.full_input_tokens, 0), 1)
                       ELSE NULL
                   END,
                   'avg_output_tokens_per_second', m.avg_output_tokens_per_second
               )
               ORDER BY m.total_tokens DESC, m.request_count DESC, m.effective_model ASC
           ) AS model_breakdown
    FROM models m
    CROSS JOIN totals t
    GROUP BY m.endpoint_id
)
SELECT grouped.label AS "label!",
       grouped.endpoint_id AS endpoint_id,
       grouped.request_count AS "request_count!",
       COALESCE(grouped.request_count::DOUBLE PRECISION / NULLIF(totals.request_count, 0), 0) AS "request_share!",
       grouped.success_count AS "success_count!",
       grouped.error_count AS "error_count!",
       grouped.total_tokens::DOUBLE PRECISION / NULLIF(totals.total_tokens, 0) AS token_share,
       grouped.cache_hit_count AS "cache_hit_count!",
       grouped.input_tokens AS "input_tokens!",
       grouped.cache_read_tokens AS "cache_read_tokens!",
       grouped.cache_write_tokens AS "cache_write_tokens!",
       grouped.output_tokens AS "output_tokens!",
       grouped.total_tokens AS "total_tokens!",
       grouped.full_input_tokens AS "full_input_tokens!",
       grouped.avg_output_tokens_per_second AS avg_output_tokens_per_second,
       model_agg.model_breakdown AS model_breakdown
FROM grouped
CROSS JOIN totals
LEFT JOIN model_agg ON model_agg.endpoint_id IS NOT DISTINCT FROM grouped.endpoint_id
ORDER BY grouped.total_tokens DESC, grouped.request_count DESC, grouped.label ASC
