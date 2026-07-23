DROP INDEX IF EXISTS idx_usage_charge_adjustments_charge;
DROP TABLE IF EXISTS usage_charge_adjustments;
DROP TABLE IF EXISTS usage_charge_lines;
DROP INDEX IF EXISTS idx_usage_charges_client_key_created_at;
DROP INDEX IF EXISTS idx_usage_charges_user_created_at;
DROP TABLE IF EXISTS usage_charges;
DROP INDEX IF EXISTS idx_billing_price_rules_cost_lookup;
DROP INDEX IF EXISTS idx_billing_price_rules_sale_lookup;
DROP TABLE IF EXISTS billing_price_rules;
DROP INDEX IF EXISTS idx_request_records_requested_model_created_at;
DROP INDEX IF EXISTS idx_request_records_client_key_created_at;
ALTER TABLE request_records
DROP COLUMN IF EXISTS upstream_model,
DROP COLUMN IF EXISTS requested_model,
DROP COLUMN IF EXISTS client_key_id;
