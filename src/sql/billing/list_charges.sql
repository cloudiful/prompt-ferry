SELECT c.charge_id AS "charge_id!", c.event_id, c.request_id AS "request_id!", c.user_id, u.login_name AS "user_login_name?",
       c.client_key_id, c.client_key_label, c.requested_model, c.upstream_model,
       c.endpoint_id, e.name AS "endpoint_name?", c.endpoint_key_id,
       c.usage_status AS "usage_status!", c.pricing_status AS "pricing_status!", c.currency AS "currency!",
       c.customer_amount,
       c.input_tokens AS "input_tokens!", c.cache_read_tokens AS "cache_read_tokens!",
       c.cache_write_tokens AS "cache_write_tokens!", c.output_tokens AS "output_tokens!",
       c.created_at AS "created_at!", c.updated_at AS "updated_at!"
FROM usage_charges c
LEFT JOIN users u ON u.user_id = c.user_id
LEFT JOIN provider_endpoints e ON e.endpoint_id = c.endpoint_id
WHERE ($1::BIGINT IS NULL OR c.user_id = $1)
  AND ($2::BIGINT IS NULL OR c.client_key_id = $2)
  AND ($3::TEXT IS NULL OR c.requested_model = $3)
  AND ($4::UUID IS NULL OR c.endpoint_id = $4)
  AND ($5::TEXT IS NULL OR c.usage_status = $5)
  AND ($6::TEXT IS NULL OR c.pricing_status = $6)
  AND ($7::UUID IS NULL OR c.request_id = $7)
  AND ($8::TIMESTAMPTZ IS NULL OR c.created_at >= $8)
  AND ($9::TIMESTAMPTZ IS NULL OR c.created_at < $9)
ORDER BY c.created_at DESC, c.charge_id DESC
LIMIT $10 OFFSET $11
