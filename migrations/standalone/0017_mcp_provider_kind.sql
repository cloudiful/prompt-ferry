-- Issue #296 Phase 1: mirror the PostgreSQL 0080 MCP provider preset column
-- for the standalone SQLite store. `provider_kind` is an optional preset id
-- (generic/minimax/context7/firecrawl); NULL keeps legacy behavior and the
-- allowed set stays enforced by shared admin validation (the column carries
-- no CHECK, matching the pre-existing standalone convention).
ALTER TABLE standalone_mcp_servers ADD COLUMN provider_kind TEXT;

UPDATE standalone_mcp_servers
SET provider_kind = 'minimax'
WHERE transport = 'builtin_minimax'
  AND provider_kind IS NULL;

UPDATE standalone_schema_meta
SET schema_version = 17
WHERE schema_key = 'standalone';
