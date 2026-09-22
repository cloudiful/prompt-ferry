-- Issue #566: mirror PostgreSQL 20260922024814 target thinking-downgrade
-- switch for the standalone SQLite store. `thinking_downgrade_enabled` is
-- INTEGER 0/1 (NOT NULL DEFAULT 0 = pre-#556 byte passthrough); frontend
-- always sends true/false (no omit semantics). Missing column on
-- pre-migration rows reads as disabled (0).
ALTER TABLE standalone_model_route_targets ADD COLUMN thinking_downgrade_enabled INTEGER NOT NULL DEFAULT 0;

UPDATE standalone_schema_meta
SET schema_version = 30
WHERE schema_key = 'standalone';
