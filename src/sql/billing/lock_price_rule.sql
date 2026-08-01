SELECT price_rule_id AS "price_rule_id!"
FROM billing_price_rules
WHERE price_rule_id = $1
FOR UPDATE
