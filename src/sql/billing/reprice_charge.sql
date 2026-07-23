UPDATE usage_charges
SET pricing_status = $2,
    provider_cost = $3,
    customer_amount = $4,
    adjusted_amount = $5,
    updated_at = NOW()
WHERE charge_id = $1
