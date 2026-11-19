-- Refresh the fixed one-minute rollup outside any metadata mutation
-- transaction. `force => true` also removes materialized buckets whose raw
-- rows were deleted or dropped by retention.
CALL public.refresh_continuous_aggregate(
    'public.request_usage_1minute',
    $1::TIMESTAMPTZ,
    $2::TIMESTAMPTZ,
    force => true
);
