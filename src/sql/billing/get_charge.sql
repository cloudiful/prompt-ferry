SELECT c.charge_id AS "charge_id!", c.event_id, c.request_id AS "request_id!", c.user_id, u.login_name AS "user_login_name?",
       c.client_key_id, c.client_key_label, c.requested_model, c.upstream_model,
       c.endpoint_id, e.name AS "endpoint_name?", c.endpoint_key_id,
       c.usage_status AS "usage_status!", c.pricing_status AS "pricing_status!", c.currency AS "currency!", c.provider_cost,
       c.customer_amount,
       c.input_tokens AS "input_tokens!", c.cache_read_tokens AS "cache_read_tokens!",
       c.cache_write_tokens AS "cache_write_tokens!", c.output_tokens AS "output_tokens!",
       c.created_at AS "created_at!", c.updated_at AS "updated_at!"
FROM usage_charges c
LEFT JOIN users u ON u.user_id = c.user_id
LEFT JOIN provider_endpoints e ON e.endpoint_id = c.endpoint_id
WHERE c.charge_id = $1
