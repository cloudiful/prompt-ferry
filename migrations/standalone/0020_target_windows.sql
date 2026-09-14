-- Issue #378 Phase I: mirror PostgreSQL 0083 effective time windows for
-- the standalone SQLite store. `active_windows` carries the normalized
-- JSON array of `{start,end}` HH:MM pairs (NULL/empty means all-day).
-- Plaintext (schedule is not a secret); validation lives in the shared
-- admin layer. Missing column on pre-migration rows reads as all-day.
ALTER TABLE standalone_model_route_targets ADD COLUMN active_windows TEXT;

UPDATE standalone_schema_meta
SET schema_version = 20
WHERE schema_key = 'standalone';
