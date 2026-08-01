INSERT INTO billing_price_rules (
    public_model, input_rate, cache_read_rate, cache_write_rate, output_rate,
    effective_from, created_by_user_id
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING price_rule_id, public_model AS "public_model!", input_rate, cache_read_rate, cache_write_rate,
          output_rate, currency,
          effective_from, effective_to, enabled, created_by_user_id, created_at, updated_at
