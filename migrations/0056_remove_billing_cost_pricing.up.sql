DELETE FROM usage_charge_lines
WHERE price_side = 'cost';

DELETE FROM billing_price_rules
WHERE price_side = 'cost';

UPDATE usage_charges
SET pricing_status = 'priced'
WHERE customer_amount IS NOT NULL;

DROP INDEX IF EXISTS idx_billing_price_rules_cost_lookup;
DROP INDEX IF EXISTS idx_billing_price_rules_sale_lookup;

ALTER TABLE billing_price_rules
    DROP CONSTRAINT IF EXISTS ck_billing_price_rules_scope,
    DROP CONSTRAINT IF EXISTS ck_billing_price_rules_side;

ALTER TABLE billing_price_rules
    DROP COLUMN IF EXISTS price_side,
    DROP COLUMN IF EXISTS endpoint_id,
    DROP COLUMN IF EXISTS upstream_model,
    ALTER COLUMN public_model SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_billing_price_rules_public_model_lookup
ON billing_price_rules(public_model, effective_from DESC)
WHERE enabled = TRUE;

ALTER TABLE usage_charges
    DROP COLUMN IF EXISTS provider_cost;

ALTER TABLE usage_charge_lines
    DROP CONSTRAINT IF EXISTS uq_usage_charge_lines_charge_side_meter,
    DROP CONSTRAINT IF EXISTS ck_usage_charge_lines_side;

ALTER TABLE usage_charge_lines
    DROP COLUMN IF EXISTS price_side;

ALTER TABLE usage_charge_lines
    ADD CONSTRAINT uq_usage_charge_lines_charge_meter UNIQUE (charge_id, meter);
