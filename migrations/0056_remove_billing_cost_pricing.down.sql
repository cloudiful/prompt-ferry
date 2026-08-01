ALTER TABLE billing_price_rules
    ALTER COLUMN public_model DROP NOT NULL;

ALTER TABLE billing_price_rules
    ADD COLUMN IF NOT EXISTS price_side TEXT,
    ADD COLUMN IF NOT EXISTS endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS upstream_model TEXT;

UPDATE billing_price_rules
SET price_side = 'sale'
WHERE price_side IS NULL;

ALTER TABLE billing_price_rules
    ALTER COLUMN price_side SET NOT NULL,
    ADD CONSTRAINT ck_billing_price_rules_side
        CHECK (price_side IN ('cost', 'sale')),
    ADD CONSTRAINT ck_billing_price_rules_scope
        CHECK (
            (price_side = 'sale' AND public_model IS NOT NULL AND endpoint_id IS NULL AND upstream_model IS NULL)
            OR (price_side = 'cost' AND public_model IS NULL AND endpoint_id IS NOT NULL AND upstream_model IS NOT NULL)
        );

DROP INDEX IF EXISTS idx_billing_price_rules_public_model_lookup;

CREATE INDEX IF NOT EXISTS idx_billing_price_rules_sale_lookup
ON billing_price_rules(public_model, effective_from DESC)
WHERE price_side = 'sale' AND enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_billing_price_rules_cost_lookup
ON billing_price_rules(endpoint_id, upstream_model, effective_from DESC)
WHERE price_side = 'cost' AND enabled = TRUE;

ALTER TABLE usage_charges
    ADD COLUMN IF NOT EXISTS provider_cost NUMERIC(30, 12);

ALTER TABLE usage_charge_lines
    DROP CONSTRAINT IF EXISTS uq_usage_charge_lines_charge_meter;

ALTER TABLE usage_charge_lines
    ADD COLUMN IF NOT EXISTS price_side TEXT;

UPDATE usage_charge_lines
SET price_side = 'sale'
WHERE price_side IS NULL;

ALTER TABLE usage_charge_lines
    ALTER COLUMN price_side SET NOT NULL,
    ADD CONSTRAINT ck_usage_charge_lines_side
        CHECK (price_side IN ('cost', 'sale')),
    ADD CONSTRAINT uq_usage_charge_lines_charge_side_meter
        UNIQUE (charge_id, price_side, meter);
