-- Issue #69 Phase P5 attempt 2, forward-only repair of the carrier function.
--
-- The first applied definition kept the window bounds in a CTE cross-joined
-- into the raw branch, so TimescaleDB could not push the range into chunk
-- exclusion or the created_at index and a short window scanned whole chunks.
-- This replaces only the function body with plain parameter predicates; the
-- continuous aggregate, its refresh policy and the metrics stay untouched.

CREATE OR REPLACE FUNCTION public.request_usage_window(
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
WITH carrier AS (
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
    -- A window inside one minute is answered from raw rows once, so the
    -- carrier must not contribute its same-minute bucket.
    WHERE NOT (
              p_start IS NOT NULL
              AND p_end IS NOT NULL
              AND p_start < p_end
              AND date_trunc('minute', p_start) = date_trunc('minute', p_end)
          )
      -- Complete interior minutes only: the leading partial minute belongs to
      -- the raw branch, and an exact-minute start keeps its own bucket.
      AND (
          p_start IS NULL
          OR v.bucket_at >= CASE
                 WHEN p_start = date_trunc('minute', p_start) THEN p_start
                 ELSE date_trunc('minute', p_start) + INTERVAL '1 minute'
             END
      )
      AND (p_end IS NULL OR v.bucket_at < date_trunc('minute', p_end))
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
    WHERE rr.event_kind = 'request'
      -- Raw rows cover exactly the leading and trailing partial minutes, and
      -- the whole window when both bounds fall inside one minute. Every range
      -- is half-open, disjoint from the carrier, and written with plain
      -- parameter predicates so TimescaleDB can push the range into chunk
      -- exclusion and the created_at index.
      AND (
          (
              p_start IS NOT NULL
              AND p_end IS NOT NULL
              AND p_start < p_end
              AND date_trunc('minute', p_start) = date_trunc('minute', p_end)
              AND rr.created_at >= p_start
              AND rr.created_at < p_end
          )
          OR (
              NOT (
                  p_start IS NOT NULL
                  AND p_end IS NOT NULL
                  AND p_start < p_end
                  AND date_trunc('minute', p_start) = date_trunc('minute', p_end)
              )
              AND p_start IS NOT NULL
              AND p_start <> date_trunc('minute', p_start)
              AND rr.created_at >= p_start
              AND rr.created_at < date_trunc('minute', p_start) + INTERVAL '1 minute'
          )
          OR (
              NOT (
                  p_start IS NOT NULL
                  AND p_end IS NOT NULL
                  AND p_start < p_end
                  AND date_trunc('minute', p_start) = date_trunc('minute', p_end)
              )
              AND p_end IS NOT NULL
              AND p_end <> date_trunc('minute', p_end)
              AND rr.created_at >= date_trunc('minute', p_end)
              AND rr.created_at < p_end
          )
      )
      AND (p_start IS NULL OR p_end IS NULL OR p_start < p_end)
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
