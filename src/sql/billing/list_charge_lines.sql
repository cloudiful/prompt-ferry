SELECT line_id, charge_id, price_side, meter, token_count, unit_rate, amount, price_rule_id, created_at
FROM usage_charge_lines
WHERE charge_id = $1
ORDER BY price_side, meter
