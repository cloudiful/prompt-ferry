-- Issue #392 Phase K: mirror PostgreSQL 0085 developer->system normalize
-- switch for the standalone SQLite store. `dev_system_normalize` is
-- INTEGER 0/1 (NOT NULL DEFAULT 0 = passthrough unchanged); frontend
-- always sends true/false (no omit semantics). Missing column on
-- pre-migration rows reads as disabled (0).
ALTER TABLE standalone_model_route_targets ADD COLUMN dev_system_normalize INTEGER NOT NULL DEFAULT 0;

UPDATE standalone_schema_meta
SET schema_version = 22
WHERE schema_key = 'standalone';
