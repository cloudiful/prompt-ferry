UPDATE billing_price_rules
SET enabled = $2, updated_at = NOW()
WHERE price_rule_id = $1
RETURNING price_rule_id, public_model AS "public_model!", input_rate, cache_read_rate, cache_write_rate,
          output_rate, currency,
          effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
