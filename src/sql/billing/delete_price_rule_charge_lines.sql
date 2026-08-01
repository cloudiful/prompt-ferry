DELETE FROM usage_charge_lines
WHERE charge_id IN (
    SELECT charge_id
    FROM usage_charge_lines
    WHERE price_rule_id = $1
)
