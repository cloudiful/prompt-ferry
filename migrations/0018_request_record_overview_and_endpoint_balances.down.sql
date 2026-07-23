DROP INDEX IF EXISTS idx_endpoint_balance_snapshots_checked_at;
DROP TABLE IF EXISTS endpoint_balance_snapshots;

DROP INDEX IF EXISTS idx_request_records_failure_family_created_at;
DROP INDEX IF EXISTS idx_request_records_token_slot_created_at;
DROP INDEX IF EXISTS idx_request_records_model_created_at;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_mcp_bearer_token_slot;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_failure_family;

ALTER TABLE request_records
DROP COLUMN IF EXISTS route_balance_share,
DROP COLUMN IF EXISTS route_balance_unit,
DROP COLUMN IF EXISTS route_balance_value,
DROP COLUMN IF EXISTS route_selection_reason,
DROP COLUMN IF EXISTS mcp_bearer_token_slot,
DROP COLUMN IF EXISTS failure_family;
