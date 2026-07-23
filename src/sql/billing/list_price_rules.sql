SELECT price_rule_id, price_side, public_model, endpoint_id, upstream_model,
       input_rate, cache_read_rate, cache_write_rate, output_rate, currency,
       effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
FROM billing_price_rules
ORDER BY price_side, COALESCE(public_model, upstream_model), effective_from DESC, created_at DESC
LIMIT $1 OFFSET $2
