-- Issue #414 Phase 2: MCP quota groups never existed on standalone SQLite
-- (credentials live as encrypted bearer tokens on standalone_mcp_servers).
-- No schema change; bump the version so PG and standalone stay in lockstep.
UPDATE standalone_schema_meta
SET schema_version = 24
WHERE schema_key = 'standalone';
