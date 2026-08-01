ALTER TABLE usage_charges
    ADD COLUMN IF NOT EXISTS input_tokens BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS output_tokens BIGINT NOT NULL DEFAULT 0;

UPDATE usage_charges c
SET input_tokens = COALESCE(lines.input_tokens, 0),
    cache_read_tokens = COALESCE(lines.cache_read_tokens, 0),
    cache_write_tokens = COALESCE(lines.cache_write_tokens, 0),
    output_tokens = COALESCE(lines.output_tokens, 0)
FROM (
    SELECT charge_id,
           MAX(token_count) FILTER (WHERE meter = 'input') AS input_tokens,
           MAX(token_count) FILTER (WHERE meter = 'cache_read') AS cache_read_tokens,
           MAX(token_count) FILTER (WHERE meter = 'cache_write') AS cache_write_tokens,
           MAX(token_count) FILTER (WHERE meter = 'output') AS output_tokens
    FROM usage_charge_lines
    GROUP BY charge_id
) lines
WHERE c.charge_id = lines.charge_id;

UPDATE usage_charges
SET customer_amount = adjusted_amount,
    pricing_status = 'priced'
WHERE pricing_status = 'adjusted';

ALTER TABLE usage_charges
    DROP CONSTRAINT IF EXISTS ck_usage_charges_pricing_status;

ALTER TABLE usage_charges
    ADD CONSTRAINT ck_usage_charges_pricing_status
    CHECK (pricing_status IN ('priced', 'unpriced'));

DROP INDEX IF EXISTS idx_usage_charge_adjustments_charge;
DROP TABLE IF EXISTS usage_charge_adjustments;

ALTER TABLE usage_charges
    DROP COLUMN IF EXISTS adjusted_amount;

CREATE INDEX IF NOT EXISTS idx_usage_charge_lines_price_rule_id
ON usage_charge_lines(price_rule_id);
