INSERT INTO billing_price_rules (
    price_side, public_model, endpoint_id, upstream_model,
    input_rate, cache_read_rate, cache_write_rate, output_rate,
    effective_from, created_by_user_id
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
RETURNING price_rule_id, price_side, public_model, endpoint_id, upstream_model,
          input_rate, cache_read_rate, cache_write_rate, output_rate, currency,
          effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
