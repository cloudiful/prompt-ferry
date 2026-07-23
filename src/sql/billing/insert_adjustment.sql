INSERT INTO usage_charge_adjustments (charge_id, amount, reason, created_by_user_id)
VALUES ($1, $2, $3, $4)
RETURNING adjustment_id, charge_id, amount, reason, created_by_user_id, created_at
