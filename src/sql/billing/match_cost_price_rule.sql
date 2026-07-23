SELECT price_rule_id, price_side, public_model, endpoint_id, upstream_model,
       input_rate, cache_read_rate, cache_write_rate, output_rate, currency,
       effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
FROM billing_price_rules
WHERE price_side = 'cost'
  AND enabled = TRUE
  AND endpoint_id = $1
  AND upstream_model = $2
  AND effective_from <= $3
  AND (effective_to IS NULL OR effective_to > $3)
ORDER BY effective_from DESC, created_at DESC
LIMIT 1
