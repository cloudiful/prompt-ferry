-- Issue #414 Phase 2: remove MCP quota groups. Drop the budget ledger tables
-- and the credential group link; credentials themselves stay for token sync.
DROP INDEX IF EXISTS idx_mcp_credentials_group;
ALTER TABLE mcp_credentials DROP COLUMN IF EXISTS quota_group_id;
DROP TABLE IF EXISTS mcp_quota_reservations;
DROP TABLE IF EXISTS mcp_quota_accounts;
DROP TABLE IF EXISTS mcp_quota_groups;
