-- Issue #69 Phase P5: one-minute request usage rollup.
--
-- The continuous aggregate stores additive per-row measures for the exact
-- dimensions the twelve statistical consumers filter or group by. Business
-- formulas (fold-aware token normalization, success/error predicates, TPS
-- terms) are evaluated per request row before aggregation, so regrouping never
-- re-derives a guard on an aggregate SUM.
CREATE MATERIALIZED VIEW public.request_usage_1minute
WITH (timescaledb.continuous, timescaledb.materialized_only = false)
AS
SELECT
    time_bucket(INTERVAL '1 minute', rr.created_at) AS bucket_at,
    rr.user_id,
    rr.request_category,
    rr.endpoint_id,
    rr.mcp_server_id,
    rr.mcp_server_name,
    rr.model,
    rr.upstream_model,
    rr.mcp_protocol_method,
    rr.ok,
    rr.request_state,
    COUNT(*)::BIGINT AS request_count,
    SUM(CASE WHEN rr.ok IS TRUE THEN 1 ELSE 0 END)::BIGINT AS success_by_ok,
    SUM(CASE WHEN rr.ok IS FALSE OR rr.request_state IN ('failed', 'aborted') THEN 1 ELSE 0 END)::BIGINT AS error_by_ok,
    SUM(CASE WHEN rr.request_state = 'completed' THEN 1 ELSE 0 END)::BIGINT AS success_by_state,
    SUM(CASE WHEN rr.request_state IN ('failed', 'aborted') THEN 1 ELSE 0 END)::BIGINT AS error_by_state,
    SUM(CASE WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0 THEN 1 ELSE 0 END)::BIGINT AS cache_hit_count,
    COALESCE(SUM(
        CASE
            WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
            THEN GREATEST(
                COALESCE(rr.input_tokens, 0)
                    - GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    - GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                0
            )
            ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
        END
    ), 0)::BIGINT AS input_tokens,
    COALESCE(SUM(GREATEST(COALESCE(rr.input_tokens, 0), 0)), 0)::BIGINT AS input_tokens_raw,
    COALESCE(SUM(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)), 0)::BIGINT AS cache_read_tokens,
    COALESCE(SUM(GREATEST(COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0), 0)), 0)::BIGINT AS cache_read_tokens_clamped,
    COALESCE(SUM(COALESCE(rr.cache_write_tokens, 0)), 0)::BIGINT AS cache_write_tokens,
    COALESCE(SUM(GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)), 0)::BIGINT AS cache_write_tokens_clamped,
    COALESCE(SUM(COALESCE(rr.output_tokens, 0)), 0)::BIGINT AS output_tokens,
    COALESCE(SUM(GREATEST(COALESCE(rr.output_tokens, 0), 0)), 0)::BIGINT AS output_tokens_clamped,
    COALESCE(SUM(GREATEST(COALESCE(rr.total_tokens, 0), 0)), 0)::BIGINT AS total_tokens,
    COALESCE(SUM(GREATEST(COALESCE(rr.cached_tokens, 0), 0)), 0)::BIGINT AS cached_tokens,
    COALESCE(SUM(
        CASE
            WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
            THEN GREATEST(
                COALESCE(rr.input_tokens, 0),
                GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                0
            )
            ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
                + GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)
        END
    ), 0)::BIGINT AS full_input_tokens,
    COALESCE(SUM(CASE WHEN rr.duration_ms IS NOT NULL THEN rr.duration_ms ELSE 0 END), 0)::BIGINT AS duration_sum_ms,
    SUM(CASE WHEN rr.duration_ms IS NOT NULL THEN 1 ELSE 0 END)::BIGINT AS duration_count,
    COALESCE(SUM(CASE WHEN rr.ttft_ms IS NOT NULL THEN rr.ttft_ms ELSE 0 END), 0)::BIGINT AS ttft_sum_ms,
    SUM(CASE WHEN rr.ttft_ms IS NOT NULL THEN 1 ELSE 0 END)::BIGINT AS ttft_count,
    COALESCE(SUM(
        CASE
            WHEN rr.request_category = 'ai'
                AND rr.request_state = 'completed'
                AND COALESCE(rr.output_tokens, 0) > 0
                AND COALESCE(rr.duration_ms, 0) > 0
            THEN rr.output_tokens::NUMERIC / rr.duration_ms::NUMERIC * 1000.0
            ELSE 0::NUMERIC
        END
    ), 0)::NUMERIC AS tps_sum,
    SUM(
        CASE
            WHEN rr.request_category = 'ai'
                AND rr.request_state = 'completed'
                AND COALESCE(rr.output_tokens, 0) > 0
                AND COALESCE(rr.duration_ms, 0) > 0
            THEN 1
            ELSE 0
        END
    )::BIGINT AS tps_count
FROM public.request_records rr
WHERE rr.event_kind = 'request'
GROUP BY bucket_at, rr.user_id, rr.request_category, rr.endpoint_id, rr.mcp_server_id,
         rr.mcp_server_name, rr.model, rr.upstream_model, rr.mcp_protocol_method,
         rr.ok, rr.request_state
WITH NO DATA;

SELECT add_continuous_aggregate_policy(
    'public.request_usage_1minute',
    start_offset => NULL,
    end_offset => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute'
);

-- One stable carrier for the twelve statistical consumers. Complete interior
-- minutes come from the real-time continuous aggregate; the leading and
-- trailing partial minutes (and a short same-minute window) are aggregated
-- directly from request rows with the identical per-row formulas. All objects
-- are public-qualified and the function is invoker-security, non-STRICT and
-- STABLE with no SET/proconfig/search_path clause.
CREATE FUNCTION public.request_usage_window(
    p_visible_user_id BIGINT,
    p_request_category TEXT,
    p_start TIMESTAMPTZ,
    p_end TIMESTAMPTZ,
    p_mcp_scope TEXT
)
RETURNS TABLE (
    bucket_at TIMESTAMPTZ,
    user_id BIGINT,
    request_category TEXT,
    endpoint_id UUID,
    mcp_server_id UUID,
    mcp_server_name TEXT,
    model TEXT,
    upstream_model TEXT,
    mcp_protocol_method TEXT,
    ok BOOLEAN,
    request_state TEXT,
    request_count BIGINT,
    success_by_ok BIGINT,
    error_by_ok BIGINT,
    success_by_state BIGINT,
    error_by_state BIGINT,
    cache_hit_count BIGINT,
    input_tokens BIGINT,
    input_tokens_raw BIGINT,
    cache_read_tokens BIGINT,
    cache_read_tokens_clamped BIGINT,
    cache_write_tokens BIGINT,
    cache_write_tokens_clamped BIGINT,
    output_tokens BIGINT,
    output_tokens_clamped BIGINT,
    total_tokens BIGINT,
    cached_tokens BIGINT,
    full_input_tokens BIGINT,
    duration_sum_ms BIGINT,
    duration_count BIGINT,
    ttft_sum_ms BIGINT,
    ttft_count BIGINT,
    tps_sum NUMERIC,
    tps_count BIGINT
)
LANGUAGE sql
STABLE
AS $$
WITH bounds AS (
    SELECT
        p_start AS start_at,
        p_end AS end_at,
        CASE WHEN p_start IS NULL THEN NULL ELSE date_trunc('minute', p_start) END AS start_floor,
        CASE
            WHEN p_start IS NULL OR p_start = date_trunc('minute', p_start) THEN NULL
            ELSE date_trunc('minute', p_start) + INTERVAL '1 minute'
        END AS leading_end,
        CASE WHEN p_end IS NULL THEN NULL ELSE date_trunc('minute', p_end) END AS end_floor,
        CASE
            WHEN p_end IS NULL OR p_end = date_trunc('minute', p_end) THEN NULL
            ELSE date_trunc('minute', p_end)
        END AS trailing_start
), short_window AS (
    SELECT
        p_start IS NOT NULL
        AND p_end IS NOT NULL
        AND p_start < p_end
        AND date_trunc('minute', p_start) = date_trunc('minute', p_end) AS whole_raw
), carrier AS (
    SELECT
        v.bucket_at,
        v.user_id,
        v.request_category,
        v.endpoint_id,
        v.mcp_server_id,
        v.mcp_server_name,
        v.model,
        v.upstream_model,
        v.mcp_protocol_method,
        v.ok,
        v.request_state,
        v.request_count,
        v.success_by_ok,
        v.error_by_ok,
        v.success_by_state,
        v.error_by_state,
        v.cache_hit_count,
        v.input_tokens,
        v.input_tokens_raw,
        v.cache_read_tokens,
        v.cache_read_tokens_clamped,
        v.cache_write_tokens,
        v.cache_write_tokens_clamped,
        v.output_tokens,
        v.output_tokens_clamped,
        v.total_tokens,
        v.cached_tokens,
        v.full_input_tokens,
        v.duration_sum_ms,
        v.duration_count,
        v.ttft_sum_ms,
        v.ttft_count,
        v.tps_sum,
        v.tps_count
    FROM public.request_usage_1minute v
    CROSS JOIN bounds b
    CROSS JOIN short_window s
    WHERE NOT s.whole_raw
      AND (
          COALESCE(b.leading_end, b.start_floor) IS NULL
          OR v.bucket_at >= COALESCE(b.leading_end, b.start_floor)
      )
      AND (b.end_floor IS NULL OR v.bucket_at < b.end_floor)
      AND (p_visible_user_id IS NULL OR v.user_id = p_visible_user_id)
      AND (p_request_category IS NULL OR v.request_category = p_request_category)
      AND (
          p_mcp_scope = 'all'
          OR p_request_category <> 'mcp'
          OR (
              p_mcp_scope = 'service'
              AND (v.mcp_server_name IS NOT NULL OR v.mcp_server_id IS NOT NULL)
          )
          OR (
              p_mcp_scope = 'gateway'
              AND v.mcp_server_name IS NULL
              AND v.mcp_server_id IS NULL
          )
      )
), raw_rows AS (
    SELECT
        time_bucket(INTERVAL '1 minute', rr.created_at) AS bucket_at,
        rr.user_id,
        rr.request_category,
        rr.endpoint_id,
        rr.mcp_server_id,
        rr.mcp_server_name,
        rr.model,
        rr.upstream_model,
        rr.mcp_protocol_method,
        rr.ok,
        rr.request_state,
        1::BIGINT AS request_count,
        CASE WHEN rr.ok IS TRUE THEN 1 ELSE 0 END::BIGINT AS success_by_ok,
        CASE WHEN rr.ok IS FALSE OR rr.request_state IN ('failed', 'aborted') THEN 1 ELSE 0 END::BIGINT AS error_by_ok,
        CASE WHEN rr.request_state = 'completed' THEN 1 ELSE 0 END::BIGINT AS success_by_state,
        CASE WHEN rr.request_state IN ('failed', 'aborted') THEN 1 ELSE 0 END::BIGINT AS error_by_state,
        CASE WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0 THEN 1 ELSE 0 END::BIGINT AS cache_hit_count,
        (CASE
            WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
            THEN GREATEST(
                COALESCE(rr.input_tokens, 0)
                    - GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    - GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                0
            )
            ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
        END)::BIGINT AS input_tokens,
        GREATEST(COALESCE(rr.input_tokens, 0), 0)::BIGINT AS input_tokens_raw,
        COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS cache_read_tokens,
        GREATEST(COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0), 0)::BIGINT AS cache_read_tokens_clamped,
        COALESCE(rr.cache_write_tokens, 0)::BIGINT AS cache_write_tokens,
        GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)::BIGINT AS cache_write_tokens_clamped,
        COALESCE(rr.output_tokens, 0)::BIGINT AS output_tokens,
        GREATEST(COALESCE(rr.output_tokens, 0), 0)::BIGINT AS output_tokens_clamped,
        GREATEST(COALESCE(rr.total_tokens, 0), 0)::BIGINT AS total_tokens,
        GREATEST(COALESCE(rr.cached_tokens, 0), 0)::BIGINT AS cached_tokens,
        (CASE
            WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                AND COALESCE(rr.input_tokens, 0) >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
            THEN GREATEST(
                COALESCE(rr.input_tokens, 0),
                GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                    + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                0
            )
            ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
                + GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)
        END)::BIGINT AS full_input_tokens,
        COALESCE(rr.duration_ms, 0)::BIGINT AS duration_sum_ms,
        CASE WHEN rr.duration_ms IS NOT NULL THEN 1 ELSE 0 END::BIGINT AS duration_count,
        COALESCE(rr.ttft_ms, 0)::BIGINT AS ttft_sum_ms,
        CASE WHEN rr.ttft_ms IS NOT NULL THEN 1 ELSE 0 END::BIGINT AS ttft_count,
        (CASE
            WHEN rr.request_category = 'ai'
                AND rr.request_state = 'completed'
                AND COALESCE(rr.output_tokens, 0) > 0
                AND COALESCE(rr.duration_ms, 0) > 0
            THEN rr.output_tokens::NUMERIC / rr.duration_ms::NUMERIC * 1000.0
            ELSE 0::NUMERIC
        END) AS tps_sum,
        CASE
            WHEN rr.request_category = 'ai'
                AND rr.request_state = 'completed'
                AND COALESCE(rr.output_tokens, 0) > 0
                AND COALESCE(rr.duration_ms, 0) > 0
            THEN 1
            ELSE 0
        END::BIGINT AS tps_count
    FROM public.request_records rr
    CROSS JOIN bounds b
    CROSS JOIN short_window s
    WHERE rr.event_kind = 'request'
      AND (
          (
              s.whole_raw
              AND rr.created_at >= b.start_at
              AND rr.created_at < b.end_at
          )
          OR (
              NOT s.whole_raw
              AND (
                  (
                      b.leading_end IS NOT NULL
                      AND rr.created_at >= b.start_at
                      AND rr.created_at < b.leading_end
                  )
                  OR (
                      b.trailing_start IS NOT NULL
                      AND rr.created_at >= b.trailing_start
                      AND rr.created_at < b.end_at
                  )
              )
          )
      )
      AND (p_visible_user_id IS NULL OR rr.user_id = p_visible_user_id)
      AND (p_request_category IS NULL OR rr.request_category = p_request_category)
      AND (
          p_mcp_scope = 'all'
          OR p_request_category <> 'mcp'
          OR (
              p_mcp_scope = 'service'
              AND (rr.mcp_server_name IS NOT NULL OR rr.mcp_server_id IS NOT NULL)
          )
          OR (
              p_mcp_scope = 'gateway'
              AND rr.mcp_server_name IS NULL
              AND rr.mcp_server_id IS NULL
          )
      )
), raw_grouped AS (
    SELECT
        bucket_at,
        user_id,
        request_category,
        endpoint_id,
        mcp_server_id,
        mcp_server_name,
        model,
        upstream_model,
        mcp_protocol_method,
        ok,
        request_state,
        SUM(request_count)::BIGINT AS request_count,
        SUM(success_by_ok)::BIGINT AS success_by_ok,
        SUM(error_by_ok)::BIGINT AS error_by_ok,
        SUM(success_by_state)::BIGINT AS success_by_state,
        SUM(error_by_state)::BIGINT AS error_by_state,
        SUM(cache_hit_count)::BIGINT AS cache_hit_count,
        SUM(input_tokens)::BIGINT AS input_tokens,
        SUM(input_tokens_raw)::BIGINT AS input_tokens_raw,
        SUM(cache_read_tokens)::BIGINT AS cache_read_tokens,
        SUM(cache_read_tokens_clamped)::BIGINT AS cache_read_tokens_clamped,
        SUM(cache_write_tokens)::BIGINT AS cache_write_tokens,
        SUM(cache_write_tokens_clamped)::BIGINT AS cache_write_tokens_clamped,
        SUM(output_tokens)::BIGINT AS output_tokens,
        SUM(output_tokens_clamped)::BIGINT AS output_tokens_clamped,
        SUM(total_tokens)::BIGINT AS total_tokens,
        SUM(cached_tokens)::BIGINT AS cached_tokens,
        SUM(full_input_tokens)::BIGINT AS full_input_tokens,
        SUM(duration_sum_ms)::BIGINT AS duration_sum_ms,
        SUM(duration_count)::BIGINT AS duration_count,
        SUM(ttft_sum_ms)::BIGINT AS ttft_sum_ms,
        SUM(ttft_count)::BIGINT AS ttft_count,
        SUM(tps_sum)::NUMERIC AS tps_sum,
        SUM(tps_count)::BIGINT AS tps_count
    FROM raw_rows
    GROUP BY bucket_at, user_id, request_category, endpoint_id, mcp_server_id,
             mcp_server_name, model, upstream_model, mcp_protocol_method, ok,
             request_state
)
SELECT * FROM carrier
UNION ALL
SELECT * FROM raw_grouped
$$;
