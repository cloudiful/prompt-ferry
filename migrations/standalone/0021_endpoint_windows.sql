-- Issue #392 Phase K: mirror PostgreSQL 0084 effective time windows for
-- the standalone SQLite store. `active_windows` carries the normalized
-- JSON array of `{start,end}` HH:MM pairs (NULL/empty means all-day).
-- Plaintext (schedule is not a secret); validation lives in the shared
-- admin layer. Missing column on pre-migration rows reads as all-day.
-- Effective windows resolve as target-nonempty else endpoint else all-day.
ALTER TABLE standalone_provider_endpoints ADD COLUMN active_windows TEXT;

UPDATE standalone_schema_meta
SET schema_version = 21
WHERE schema_key = 'standalone';
