DROP INDEX IF EXISTS idx_request_records_model_route_rule_created_at;
DROP INDEX IF EXISTS idx_request_records_mcp_server_created_at;
DROP INDEX IF EXISTS idx_request_records_category_created_at;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_category;

ALTER TABLE request_records
DROP COLUMN IF EXISTS mcp_operation_name,
DROP COLUMN IF EXISTS mcp_protocol_method,
DROP COLUMN IF EXISTS mcp_server_name,
DROP COLUMN IF EXISTS mcp_server_id,
DROP COLUMN IF EXISTS model_route_rule_id,
DROP COLUMN IF EXISTS request_category;

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_monthly_max_requests;

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_daily_max_requests;

ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS monthly_max_requests,
DROP COLUMN IF EXISTS daily_max_requests;

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_monthly_max_requests;

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_daily_max_requests;

ALTER TABLE model_endpoint_rules
DROP COLUMN IF EXISTS monthly_max_requests,
DROP COLUMN IF EXISTS daily_max_requests;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_monthly_max_requests;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_daily_max_requests;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS monthly_max_requests,
DROP COLUMN IF EXISTS daily_max_requests;
