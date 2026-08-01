UPDATE usage_charges
SET pricing_status = 'unpriced',
    provider_cost = NULL,
    customer_amount = NULL,
    updated_at = NOW()
WHERE charge_id IN (
    SELECT charge_id
    FROM usage_charge_lines
    WHERE price_rule_id = $1
)
