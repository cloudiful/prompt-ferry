SELECT adjustment_id, charge_id, amount, reason, created_by_user_id, created_at
FROM usage_charge_adjustments
WHERE charge_id = $1
ORDER BY created_at ASC, adjustment_id ASC
