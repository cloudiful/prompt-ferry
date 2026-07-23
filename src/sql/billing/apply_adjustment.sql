UPDATE usage_charges
SET adjusted_amount = COALESCE(customer_amount, 0) + $2,
    pricing_status = 'adjusted',
    updated_at = NOW()
WHERE charge_id = $1
