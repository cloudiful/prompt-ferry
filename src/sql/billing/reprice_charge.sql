UPDATE usage_charges
SET pricing_status = $2,
    customer_amount = $3,
    updated_at = NOW()
WHERE charge_id = $1
