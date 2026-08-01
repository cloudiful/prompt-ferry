SELECT price_rule_id, public_model AS "public_model!",
       input_rate, cache_read_rate, cache_write_rate, output_rate, currency,
       effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
FROM billing_price_rules
WHERE enabled = TRUE
  AND public_model = $1
  AND effective_from <= $2
  AND (effective_to IS NULL OR effective_to > $2)
ORDER BY effective_from DESC, created_at DESC
LIMIT 1
