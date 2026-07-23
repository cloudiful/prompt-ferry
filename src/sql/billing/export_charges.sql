SELECT c.charge_id AS "charge_id!", c.request_id AS "request_id!", u.login_name AS user_login_name, c.client_key_label,
       c.requested_model, c.upstream_model, e.name AS endpoint_name,
       c.usage_status AS "usage_status!", c.pricing_status AS "pricing_status!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'input'), 0)::BIGINT AS "input_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'cache_read'), 0)::BIGINT AS "cache_read_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'cache_write'), 0)::BIGINT AS "cache_write_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'output'), 0)::BIGINT AS "output_tokens!",
       c.provider_cost, c.customer_amount, c.adjusted_amount, c.created_at AS "created_at!"
FROM usage_charges c
LEFT JOIN users u ON u.user_id = c.user_id
LEFT JOIN provider_endpoints e ON e.endpoint_id = c.endpoint_id
LEFT JOIN usage_charge_lines l ON l.charge_id = c.charge_id
WHERE ($1::BIGINT IS NULL OR c.user_id = $1)
  AND ($2::BIGINT IS NULL OR c.client_key_id = $2)
  AND ($3::TEXT IS NULL OR c.requested_model = $3)
  AND ($4::UUID IS NULL OR c.endpoint_id = $4)
  AND ($5::TEXT IS NULL OR c.usage_status = $5)
  AND ($6::TEXT IS NULL OR c.pricing_status = $6)
  AND ($7::UUID IS NULL OR c.request_id = $7)
  AND ($8::TIMESTAMPTZ IS NULL OR c.created_at >= $8)
  AND ($9::TIMESTAMPTZ IS NULL OR c.created_at < $9)
GROUP BY c.charge_id, u.login_name, e.name
ORDER BY c.created_at ASC, c.charge_id ASC
