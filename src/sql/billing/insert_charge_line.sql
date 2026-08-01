INSERT INTO usage_charge_lines (
    charge_id, meter, token_count, unit_rate, amount, price_rule_id
)
VALUES ($1, $2, $3, $4, $5, $6)
