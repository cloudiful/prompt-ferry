SELECT c.charge_id AS "charge_id!", c.requested_model, c.upstream_model, c.endpoint_id,
       c.created_at AS "created_at!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'input'), 0)::BIGINT AS "input_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'cache_read'), 0)::BIGINT AS "cache_read_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'cache_write'), 0)::BIGINT AS "cache_write_tokens!",
       COALESCE(MAX(l.token_count) FILTER (WHERE l.price_side = 'sale' AND l.meter = 'output'), 0)::BIGINT AS "output_tokens!"
FROM usage_charges c
LEFT JOIN usage_charge_lines l ON l.charge_id = c.charge_id
WHERE c.usage_status = 'known' AND c.pricing_status = 'unpriced'
GROUP BY c.charge_id
ORDER BY c.created_at ASC
LIMIT $1
