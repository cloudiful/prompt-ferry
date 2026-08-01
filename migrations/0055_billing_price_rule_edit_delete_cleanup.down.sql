DROP INDEX IF EXISTS idx_usage_charge_lines_price_rule_id;

ALTER TABLE usage_charges
    ADD COLUMN IF NOT EXISTS adjusted_amount NUMERIC(30, 12);

UPDATE usage_charges
SET adjusted_amount = customer_amount;

ALTER TABLE usage_charges
    DROP CONSTRAINT IF EXISTS ck_usage_charges_pricing_status;

ALTER TABLE usage_charges
    ADD CONSTRAINT ck_usage_charges_pricing_status
    CHECK (pricing_status IN ('priced', 'unpriced', 'adjusted'));

CREATE TABLE IF NOT EXISTS usage_charge_adjustments (
    adjustment_id BIGSERIAL PRIMARY KEY,
    charge_id BIGINT NOT NULL REFERENCES usage_charges(charge_id) ON DELETE RESTRICT,
    amount NUMERIC(30, 12) NOT NULL,
    reason TEXT NOT NULL,
    created_by_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_usage_charge_adjustments_charge
ON usage_charge_adjustments(charge_id, created_at ASC);

ALTER TABLE usage_charges
    DROP COLUMN IF EXISTS input_tokens,
    DROP COLUMN IF EXISTS cache_read_tokens,
    DROP COLUMN IF EXISTS cache_write_tokens,
    DROP COLUMN IF EXISTS output_tokens;
