ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS daily_max_requests INTEGER,
ADD COLUMN IF NOT EXISTS monthly_max_requests INTEGER;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_daily_max_requests;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_daily_max_requests
CHECK (daily_max_requests IS NULL OR daily_max_requests > 0);

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_monthly_max_requests;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_monthly_max_requests
CHECK (monthly_max_requests IS NULL OR monthly_max_requests > 0);

ALTER TABLE model_endpoint_rules
ADD COLUMN IF NOT EXISTS daily_max_requests INTEGER,
ADD COLUMN IF NOT EXISTS monthly_max_requests INTEGER;

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_daily_max_requests;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_daily_max_requests
CHECK (daily_max_requests IS NULL OR daily_max_requests > 0);

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_monthly_max_requests;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_monthly_max_requests
CHECK (monthly_max_requests IS NULL OR monthly_max_requests > 0);

ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS daily_max_requests INTEGER,
ADD COLUMN IF NOT EXISTS monthly_max_requests INTEGER;

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_daily_max_requests;

ALTER TABLE mcp_servers
ADD CONSTRAINT ck_mcp_servers_daily_max_requests
CHECK (daily_max_requests IS NULL OR daily_max_requests > 0);

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_monthly_max_requests;

ALTER TABLE mcp_servers
ADD CONSTRAINT ck_mcp_servers_monthly_max_requests
CHECK (monthly_max_requests IS NULL OR monthly_max_requests > 0);

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS request_category TEXT NOT NULL DEFAULT 'ai',
ADD COLUMN IF NOT EXISTS model_route_rule_id UUID REFERENCES model_endpoint_rules(rule_id) ON DELETE SET NULL,
ADD COLUMN IF NOT EXISTS mcp_server_id UUID REFERENCES mcp_servers(server_id) ON DELETE SET NULL,
ADD COLUMN IF NOT EXISTS mcp_server_name TEXT,
ADD COLUMN IF NOT EXISTS mcp_protocol_method TEXT,
ADD COLUMN IF NOT EXISTS mcp_operation_name TEXT;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_category;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_request_category
CHECK (request_category IN ('ai', 'mcp'));

CREATE INDEX IF NOT EXISTS idx_request_records_category_created_at
ON request_records(request_category, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_mcp_server_created_at
ON request_records(mcp_server_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_model_route_rule_created_at
ON request_records(model_route_rule_id, created_at DESC);
