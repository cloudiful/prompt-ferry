-- Issue #430 Task1: remove daily/monthly quota columns full-chain (destructive, no rebuild).
-- PG only; standalone handled in migrations/standalone/0025_quota_cleanup.sql.
ALTER TABLE provider_endpoints DROP CONSTRAINT IF EXISTS ck_provider_endpoints_daily_max_requests;
ALTER TABLE provider_endpoints DROP CONSTRAINT IF EXISTS ck_provider_endpoints_monthly_max_requests;
ALTER TABLE provider_endpoints DROP COLUMN IF EXISTS daily_max_requests;
ALTER TABLE provider_endpoints DROP COLUMN IF EXISTS monthly_max_requests;
ALTER TABLE model_endpoint_rules DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_daily_max_requests;
ALTER TABLE model_endpoint_rules DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_monthly_max_requests;
ALTER TABLE model_endpoint_rules DROP COLUMN IF EXISTS daily_max_requests;
ALTER TABLE model_endpoint_rules DROP COLUMN IF EXISTS monthly_max_requests;
ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_servers_daily_max_requests;
ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_servers_monthly_max_requests;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS daily_max_requests;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS monthly_max_requests;
ALTER TABLE mcp_credentials DROP CONSTRAINT IF EXISTS ck_mcp_credentials_limits;
ALTER TABLE mcp_credentials DROP COLUMN IF EXISTS daily_limit;
ALTER TABLE mcp_credentials DROP COLUMN IF EXISTS monthly_limit;
