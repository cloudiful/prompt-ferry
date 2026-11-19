WITH rows AS (
    SELECT usage.*
    FROM public.request_usage_window($1::BIGINT, $2::TEXT, $3::TIMESTAMPTZ, $4::TIMESTAMPTZ, 'all') usage
    WHERE ($5::TEXT IS NULL OR COALESCE(
        (SELECT u.login_name FROM users u WHERE u.user_id = usage.user_id),
        CASE WHEN usage.user_id IS NULL THEN '-' ELSE '#' || usage.user_id::TEXT END
    ) = $5)
), normalized AS (
    SELECT COALESCE(upstream_model, model, '(unknown)') AS effective_model,
           endpoint_id,
           request_count,
           success_by_ok,
           error_by_ok,
           cache_hit_count,
           input_tokens,
           cache_read_tokens,
           cache_write_tokens,
           output_tokens,
           total_tokens,
           full_input_tokens,
           tps_sum,
           tps_count
    FROM rows
), grouped AS (
    SELECT COALESCE(pe.name, n.endpoint_id::TEXT, '(direct)') AS label,
           n.endpoint_id,
           COALESCE(SUM(n.request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(n.success_by_ok), 0)::BIGINT AS success_count,
           COALESCE(SUM(n.error_by_ok), 0)::BIGINT AS error_count,
           COALESCE(SUM(n.cache_hit_count), 0)::BIGINT AS cache_hit_count,
           COALESCE(SUM(n.input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(n.cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(n.output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(n.total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(n.full_input_tokens), 0)::BIGINT AS full_input_tokens,
           SUM(n.tps_sum)::DOUBLE PRECISION / NULLIF(SUM(n.tps_count), 0) AS avg_output_tokens_per_second
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
           COALESCE(SUM(n.request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(n.error_by_ok), 0)::BIGINT AS error_count,
           COALESCE(SUM(n.total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(n.cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.full_input_tokens), 0)::BIGINT AS full_input_tokens,
           SUM(n.tps_sum)::DOUBLE PRECISION / NULLIF(SUM(n.tps_count), 0) AS avg_output_tokens_per_second
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
