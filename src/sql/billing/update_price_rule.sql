UPDATE billing_price_rules
SET price_side = $2,
    public_model = $3,
    endpoint_id = $4,
    upstream_model = $5,
    input_rate = $6,
    cache_read_rate = $7,
    cache_write_rate = $8,
    output_rate = $9,
    effective_from = $10,
    updated_at = NOW()
WHERE price_rule_id = $1
RETURNING price_rule_id, price_side, public_model, endpoint_id, upstream_model,
          input_rate, cache_read_rate, cache_write_rate, output_rate, currency,
          effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
