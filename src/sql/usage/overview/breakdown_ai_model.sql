WITH normalized AS (
    SELECT rr.model,
           rr.ok,
           rr.request_state,
           rr.endpoint_id,
           rr.failure_family,
           rr.output_tokens AS raw_output_tokens,
           rr.duration_ms,
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
), grouped AS (
    SELECT COALESCE(model, '(unknown)') AS label,
           model,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (WHERE ok IS TRUE)::BIGINT AS success_count,
           COUNT(*) FILTER (
               WHERE ok IS FALSE OR request_state IN ('failed', 'aborted')
           )::BIGINT AS error_count,
            COUNT(*) FILTER (
                WHERE normalized_cache_read_tokens > 0
            )::BIGINT AS cache_hit_count,
           COALESCE(SUM(normalized_input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(normalized_cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           -- Issue #342: persisted closed-loop total, so per-model
           -- `token_share` and the ordering reconcile with `usage_summary`.
           COALESCE(SUM(GREATEST(total_tokens, 0)), 0)::BIGINT AS total_tokens,
           -- P1 (issue #226): aggregated `cache_rate` denominator is
           -- SUM(normalized_full_input_tokens) computed row-by-row (fold-aware),
           -- not the raw `input_tokens` SUM which double-counts the cache.
           COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS full_input_tokens,
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
), upstream AS (
    -- P1 (issue #207): per (model x endpoint_id) aggregates for the hover
    -- breakdown. Error predicate mirrors `metrics.sql`
    -- (`ok IS FALSE OR request_state IN (...)`, `failure_family` kept for
    -- observability) so the existing `ok` / `failure_family` / `endpoint_id`
    -- indexes can serve the grouping without a new migration.
    SELECT COALESCE(n.model, '(unknown)') AS model_key,
           n.endpoint_id,
           COALESCE(pe.name, n.endpoint_id::TEXT, '(direct)') AS endpoint_name,
           COUNT(*)::BIGINT AS request_count,
           COUNT(*) FILTER (
               WHERE n.ok IS FALSE OR n.request_state IN ('failed', 'aborted')
           )::BIGINT AS error_count,
           COALESCE(SUM(GREATEST(n.total_tokens, 0)), 0)::BIGINT AS total_tokens,
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
    GROUP BY COALESCE(n.model, '(unknown)'), n.endpoint_id, pe.name
), upstream_agg AS (
    SELECT model_key,
           COUNT(*)::BIGINT AS upstream_count,
           json_agg(
               json_build_object(
                   'endpoint_id', endpoint_id,
                   'endpoint_name', endpoint_name,
                   'request_count', request_count,
                   'error_count', error_count,
                   'error_rate', CASE WHEN request_count > 0 THEN error_count::DOUBLE PRECISION / request_count ELSE 0 END,
                   'total_tokens', total_tokens,
                   'avg_output_tokens_per_second', avg_output_tokens_per_second
               )
               ORDER BY total_tokens DESC, request_count DESC, endpoint_name ASC
           ) AS upstream_breakdown
    FROM upstream
    GROUP BY model_key
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
       grouped.error_count AS "error_count!",
       grouped.total_tokens::DOUBLE PRECISION / NULLIF(totals.total_tokens, 0) AS token_share,
       cache_hit_count AS "cache_hit_count!",
       input_tokens AS "input_tokens!",
       cache_read_tokens AS "cache_read_tokens!",
       cache_write_tokens AS "cache_write_tokens!",
       output_tokens AS "output_tokens!",
       grouped.total_tokens AS "total_tokens!",
       grouped.full_input_tokens AS "full_input_tokens!",
       grouped.avg_output_tokens_per_second AS avg_output_tokens_per_second,
       COALESCE(upstream_agg.upstream_count, 0)::BIGINT AS "upstream_count!",
       upstream_agg.upstream_breakdown AS upstream_breakdown
FROM grouped
CROSS JOIN totals
LEFT JOIN upstream_agg ON upstream_agg.model_key = grouped.label
ORDER BY grouped.total_tokens DESC, grouped.request_count DESC, grouped.label ASC
LIMIT 50
