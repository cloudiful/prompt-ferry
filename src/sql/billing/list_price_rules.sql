SELECT price_rule_id, public_model AS "public_model!", input_rate, cache_read_rate, cache_write_rate,
       output_rate, currency,
       effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
FROM billing_price_rules
ORDER BY public_model, effective_from DESC, created_at DESC, price_rule_id ASC
LIMIT $1 OFFSET $2
