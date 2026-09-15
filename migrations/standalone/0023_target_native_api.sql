-- Issue #409 Phase 1: mirror PostgreSQL 0086 per-target native API for
-- the standalone SQLite store. `native_api` carries the target-level port
-- type (`auto` default = follow the caller). An explicit value wins over
-- the endpoint `native_api`; `auto` falls back to endpoint then global.
-- Plaintext (not a secret); missing column on pre-migration rows reads as
-- `auto` (see `rows::route_target`).
ALTER TABLE standalone_model_route_targets ADD COLUMN native_api TEXT NOT NULL DEFAULT 'auto';

UPDATE standalone_schema_meta
SET schema_version = 23
WHERE schema_key = 'standalone';
