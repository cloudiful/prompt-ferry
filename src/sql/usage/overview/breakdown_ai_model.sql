WITH rows AS (
    SELECT usage.*
    FROM public.request_usage_window($1::BIGINT, $2::TEXT, $3::TIMESTAMPTZ, $4::TIMESTAMPTZ, 'all') usage
    WHERE ($5::TEXT IS NULL OR COALESCE(
        (SELECT u.login_name FROM users u WHERE u.user_id = usage.user_id),
        CASE WHEN usage.user_id IS NULL THEN '-' ELSE '#' || usage.user_id::TEXT END
    ) = $5)
), grouped AS (
    SELECT COALESCE(model, '(unknown)') AS label,
           model,
           COALESCE(SUM(request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(success_by_ok), 0)::BIGINT AS success_count,
           COALESCE(SUM(error_by_ok), 0)::BIGINT AS error_count,
           COALESCE(SUM(cache_hit_count), 0)::BIGINT AS cache_hit_count,
           COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(full_input_tokens), 0)::BIGINT AS full_input_tokens,
           SUM(tps_sum)::DOUBLE PRECISION / NULLIF(SUM(tps_count), 0) AS avg_output_tokens_per_second
    FROM rows
    GROUP BY model
), totals AS (
    SELECT SUM(request_count)::DOUBLE PRECISION AS request_count,
           SUM(total_tokens)::DOUBLE PRECISION AS total_tokens
    FROM grouped
), upstream AS (
    SELECT COALESCE(n.model, '(unknown)') AS model_key,
           n.endpoint_id,
           n.upstream_model,
           COALESCE(pe.name, n.endpoint_id::TEXT, '(direct)') AS endpoint_name,
           COALESCE(SUM(n.request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(n.error_by_ok), 0)::BIGINT AS error_count,
           COALESCE(SUM(n.total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(n.cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.full_input_tokens), 0)::BIGINT AS full_input_tokens,
           SUM(n.tps_sum)::DOUBLE PRECISION / NULLIF(SUM(n.tps_count), 0) AS avg_output_tokens_per_second
    FROM rows n
    LEFT JOIN provider_endpoints pe ON pe.endpoint_id = n.endpoint_id
    GROUP BY COALESCE(n.model, '(unknown)'), n.endpoint_id, n.upstream_model, pe.name
), upstream_agg AS (
    SELECT u.model_key,
           COUNT(*)::BIGINT AS upstream_count,
           json_agg(
               json_build_object(
                   'endpoint_id', u.endpoint_id,
                   'endpoint_name', u.endpoint_name,
                   'upstream_model', u.upstream_model,
                   'request_count', u.request_count,
                   'request_share', CASE WHEN t.request_count > 0 THEN u.request_count::DOUBLE PRECISION / t.request_count ELSE 0 END,
                   'error_count', u.error_count,
                   'error_rate', CASE WHEN u.request_count > 0 THEN u.error_count::DOUBLE PRECISION / u.request_count ELSE 0 END,
                   'total_tokens', u.total_tokens,
                   'token_share', u.total_tokens::DOUBLE PRECISION / NULLIF(t.total_tokens, 0),
                   'cache_rate', CASE
                       WHEN u.full_input_tokens > 0
                       THEN LEAST(GREATEST(u.cache_read_tokens::DOUBLE PRECISION / u.full_input_tokens, 0), 1)
                       ELSE NULL
                   END,
                   'avg_output_tokens_per_second', u.avg_output_tokens_per_second
               )
               ORDER BY u.total_tokens DESC, u.request_count DESC, u.endpoint_name ASC, u.upstream_model ASC NULLS LAST
           ) AS upstream_breakdown
    FROM upstream u
    CROSS JOIN totals t
    GROUP BY u.model_key
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
