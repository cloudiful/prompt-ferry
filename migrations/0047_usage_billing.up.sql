ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS client_key_id BIGINT REFERENCES client_keys(key_id) ON DELETE SET NULL,
ADD COLUMN IF NOT EXISTS requested_model TEXT,
ADD COLUMN IF NOT EXISTS upstream_model TEXT;

CREATE INDEX IF NOT EXISTS idx_request_records_client_key_created_at
ON request_records(client_key_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_requested_model_created_at
ON request_records(requested_model, created_at DESC);

CREATE TABLE IF NOT EXISTS billing_price_rules (
    price_rule_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    price_side TEXT NOT NULL,
    public_model TEXT,
    endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    upstream_model TEXT,
    input_rate NUMERIC(30, 12) NOT NULL,
    cache_read_rate NUMERIC(30, 12) NOT NULL,
    cache_write_rate NUMERIC(30, 12) NOT NULL,
    output_rate NUMERIC(30, 12) NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CNY',
    effective_from TIMESTAMPTZ NOT NULL,
    effective_to TIMESTAMPTZ,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_billing_price_rules_side CHECK (price_side IN ('cost', 'sale')),
    CONSTRAINT ck_billing_price_rules_currency CHECK (currency = 'CNY'),
    CONSTRAINT ck_billing_price_rules_scope CHECK (
        (price_side = 'sale' AND public_model IS NOT NULL AND endpoint_id IS NULL AND upstream_model IS NULL)
        OR (price_side = 'cost' AND public_model IS NULL AND endpoint_id IS NOT NULL AND upstream_model IS NOT NULL)
    ),
    CONSTRAINT ck_billing_price_rules_rates_nonnegative CHECK (
        input_rate >= 0 AND cache_read_rate >= 0 AND cache_write_rate >= 0 AND output_rate >= 0
    ),
    CONSTRAINT ck_billing_price_rules_effective_range CHECK (effective_to IS NULL OR effective_to > effective_from)
);

CREATE INDEX IF NOT EXISTS idx_billing_price_rules_sale_lookup
ON billing_price_rules(public_model, effective_from DESC)
WHERE price_side = 'sale' AND enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_billing_price_rules_cost_lookup
ON billing_price_rules(endpoint_id, upstream_model, effective_from DESC)
WHERE price_side = 'cost' AND enabled = TRUE;

CREATE TABLE IF NOT EXISTS usage_charges (
    charge_id BIGSERIAL PRIMARY KEY,
    event_id BIGINT NOT NULL UNIQUE REFERENCES request_records(event_id) ON DELETE RESTRICT,
    request_id UUID NOT NULL,
    user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    client_key_id BIGINT REFERENCES client_keys(key_id) ON DELETE SET NULL,
    client_key_label TEXT,
    requested_model TEXT,
    upstream_model TEXT,
    endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL,
    endpoint_key_id UUID REFERENCES endpoint_api_keys(key_id) ON DELETE SET NULL,
    usage_status TEXT NOT NULL,
    pricing_status TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CNY',
    provider_cost NUMERIC(30, 12),
    customer_amount NUMERIC(30, 12),
    adjusted_amount NUMERIC(30, 12),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_usage_charges_usage_status CHECK (usage_status IN ('known', 'unknown')),
    CONSTRAINT ck_usage_charges_pricing_status CHECK (pricing_status IN ('priced', 'unpriced', 'adjusted')),
    CONSTRAINT ck_usage_charges_currency CHECK (currency = 'CNY')
);

CREATE INDEX IF NOT EXISTS idx_usage_charges_user_created_at
ON usage_charges(user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_usage_charges_client_key_created_at
ON usage_charges(client_key_id, created_at DESC);

CREATE TABLE IF NOT EXISTS usage_charge_lines (
    line_id BIGSERIAL PRIMARY KEY,
    charge_id BIGINT NOT NULL REFERENCES usage_charges(charge_id) ON DELETE RESTRICT,
    price_side TEXT NOT NULL,
    meter TEXT NOT NULL,
    token_count BIGINT NOT NULL,
    unit_rate NUMERIC(30, 12) NOT NULL,
    amount NUMERIC(30, 12) NOT NULL,
    price_rule_id UUID REFERENCES billing_price_rules(price_rule_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_usage_charge_lines_side CHECK (price_side IN ('cost', 'sale')),
    CONSTRAINT ck_usage_charge_lines_meter CHECK (meter IN ('input', 'cache_read', 'cache_write', 'output')),
    CONSTRAINT ck_usage_charge_lines_token_count CHECK (token_count >= 0),
    CONSTRAINT ck_usage_charge_lines_rate_nonnegative CHECK (unit_rate >= 0),
    CONSTRAINT uq_usage_charge_lines_charge_side_meter UNIQUE (charge_id, price_side, meter)
);

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
