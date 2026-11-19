WITH rows AS (
    SELECT usage.*
    FROM public.request_usage_window($1::BIGINT, $2::TEXT, $3::TIMESTAMPTZ, $4::TIMESTAMPTZ, $6::TEXT) usage
    WHERE ($5::TEXT IS NULL OR COALESCE(
        (SELECT u.login_name FROM users u WHERE u.user_id = usage.user_id),
        CASE WHEN usage.user_id IS NULL THEN '-' ELSE '#' || usage.user_id::TEXT END
    ) = $5)
), grouped AS (
    SELECT COALESCE(n.mcp_server_name, ms.name, '(unknown)') AS label,
           n.mcp_server_id,
           ms.provider_kind AS server_provider_kind,
           CASE
               WHEN n.mcp_server_name IS NOT NULL OR n.mcp_server_id IS NOT NULL
               THEN 'service'
               ELSE 'gateway'
           END AS mcp_scope,
           COALESCE(SUM(n.request_count), 0)::BIGINT AS request_count,
           COALESCE(SUM(n.success_by_ok), 0)::BIGINT AS success_count,
           COALESCE(SUM(n.cache_hit_count), 0)::BIGINT AS cache_hit_count,
           COALESCE(SUM(n.input_tokens), 0)::BIGINT AS input_tokens,
           COALESCE(SUM(n.cache_read_tokens), 0)::BIGINT AS cache_read_tokens,
           COALESCE(SUM(n.cache_write_tokens), 0)::BIGINT AS cache_write_tokens,
           COALESCE(SUM(n.output_tokens), 0)::BIGINT AS output_tokens,
           COALESCE(SUM(n.total_tokens), 0)::BIGINT AS total_tokens,
           COALESCE(SUM(n.full_input_tokens), 0)::BIGINT AS full_input_tokens
    FROM rows n
    LEFT JOIN mcp_servers ms ON ms.server_id = n.mcp_server_id
    GROUP BY COALESCE(n.mcp_server_name, ms.name, '(unknown)'), n.mcp_server_id,
             ms.provider_kind,
             CASE
                 WHEN n.mcp_server_name IS NOT NULL OR n.mcp_server_id IS NOT NULL
                 THEN 'service'
                 ELSE 'gateway'
             END
), totals AS (
    SELECT SUM(request_count)::DOUBLE PRECISION AS request_count
    FROM grouped
)
SELECT label AS "label!",
       NULL::TEXT AS model,
       mcp_server_id,
       grouped.server_provider_kind AS "server_provider_kind",
       grouped.mcp_scope AS "mcp_scope?",
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
