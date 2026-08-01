WITH per_charge AS (
    SELECT c.charge_id, COALESCE(c.client_key_label, 'unkeyed') AS grouping_key,
           c.provider_cost, c.customer_amount,
           c.input_tokens, c.cache_read_tokens, c.cache_write_tokens, c.output_tokens
    FROM usage_charges c
    WHERE ($1::BIGINT IS NULL OR c.user_id = $1)
      AND ($2::BIGINT IS NULL OR c.client_key_id = $2)
      AND ($3::TEXT IS NULL OR c.requested_model = $3)
      AND ($4::UUID IS NULL OR c.endpoint_id = $4)
      AND ($5::TEXT IS NULL OR c.usage_status = $5)
      AND ($6::TEXT IS NULL OR c.pricing_status = $6)
      AND ($7::UUID IS NULL OR c.request_id = $7)
      AND ($8::TIMESTAMPTZ IS NULL OR c.created_at >= $8)
      AND ($9::TIMESTAMPTZ IS NULL OR c.created_at < $9)
)
SELECT grouping_key AS "grouping_key!", COUNT(*)::BIGINT AS "request_count!",
       SUM(input_tokens)::BIGINT AS "input_tokens!", SUM(cache_read_tokens)::BIGINT AS "cache_read_tokens!",
       SUM(cache_write_tokens)::BIGINT AS "cache_write_tokens!", SUM(output_tokens)::BIGINT AS "output_tokens!",
       COALESCE(SUM(provider_cost), 0)::NUMERIC AS "provider_cost!",
       COALESCE(SUM(customer_amount), 0)::NUMERIC AS "customer_amount!"
FROM per_charge
GROUP BY grouping_key
ORDER BY SUM(customer_amount) DESC, grouping_key
LIMIT $10
