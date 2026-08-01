UPDATE billing_price_rules
SET public_model = $2,
    input_rate = $3,
    cache_read_rate = $4,
    cache_write_rate = $5,
    output_rate = $6,
    effective_from = $7,
    updated_at = NOW()
WHERE price_rule_id = $1
RETURNING price_rule_id, public_model AS "public_model!", input_rate, cache_read_rate, cache_write_rate,
          output_rate, currency,
          effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
