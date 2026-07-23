WITH per_charge AS (
    SELECT c.charge_id, COALESCE(c.requested_model, 'unknown') AS grouping_key,
           c.provider_cost, c.adjusted_amount,
           COALESCE(MAX(i.token_count) FILTER (WHERE i.price_side = 'sale' AND i.meter = 'input'), 0)::BIGINT AS input_tokens,
           COALESCE(MAX(i.token_count) FILTER (WHERE i.price_side = 'sale' AND i.meter = 'cache_read'), 0)::BIGINT AS cache_read_tokens,
           COALESCE(MAX(i.token_count) FILTER (WHERE i.price_side = 'sale' AND i.meter = 'cache_write'), 0)::BIGINT AS cache_write_tokens,
           COALESCE(MAX(i.token_count) FILTER (WHERE i.price_side = 'sale' AND i.meter = 'output'), 0)::BIGINT AS output_tokens
    FROM usage_charges c
    LEFT JOIN usage_charge_lines i ON i.charge_id = c.charge_id
    WHERE ($1::BIGINT IS NULL OR c.user_id = $1)
      AND ($2::BIGINT IS NULL OR c.client_key_id = $2)
      AND ($3::TEXT IS NULL OR c.requested_model = $3)
      AND ($4::UUID IS NULL OR c.endpoint_id = $4)
      AND ($5::TEXT IS NULL OR c.usage_status = $5)
      AND ($6::TEXT IS NULL OR c.pricing_status = $6)
      AND ($7::UUID IS NULL OR c.request_id = $7)
      AND ($8::TIMESTAMPTZ IS NULL OR c.created_at >= $8)
      AND ($9::TIMESTAMPTZ IS NULL OR c.created_at < $9)
    GROUP BY c.charge_id
)
SELECT grouping_key AS "grouping_key!", COUNT(*)::BIGINT AS "request_count!",
       SUM(input_tokens)::BIGINT AS "input_tokens!", SUM(cache_read_tokens)::BIGINT AS "cache_read_tokens!",
       SUM(cache_write_tokens)::BIGINT AS "cache_write_tokens!", SUM(output_tokens)::BIGINT AS "output_tokens!",
       COALESCE(SUM(provider_cost), 0)::NUMERIC AS "provider_cost!",
       COALESCE(SUM(adjusted_amount), 0)::NUMERIC AS "adjusted_amount!"
FROM per_charge
GROUP BY grouping_key
ORDER BY SUM(adjusted_amount) DESC, grouping_key
LIMIT $10
