-- Issue #464: mirror PostgreSQL 0090 per-target thinking effort override
-- for the standalone SQLite store. `thinking_effort_override` is TEXT NULL;
-- NULL/empty means inherit (follow the caller); an explicit value is one of
-- none/minimal/low/medium/high/xhigh/max and force-replaces the caller
-- value on Chat (`reasoning_effort`) and Responses (`reasoning.effort`).
-- Plaintext (not a secret); missing column on pre-migration rows reads as
-- inherit (see `rows::route_target`).
ALTER TABLE standalone_model_route_targets
ADD COLUMN thinking_effort_override TEXT NULL;

UPDATE standalone_schema_meta
SET schema_version = 26
WHERE schema_key = 'standalone';
