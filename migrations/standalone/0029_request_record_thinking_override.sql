-- Issue #546: mirror PostgreSQL 20260921175626 per-request thinking effort
-- override snapshot for the standalone SQLite store. `applied_thinking_effort_override`
-- is TEXT NULL; NULL means the standalone summary carried no snapshot.
ALTER TABLE standalone_usage_summaries
ADD COLUMN applied_thinking_effort_override TEXT NULL;

UPDATE standalone_schema_meta
SET schema_version = 29
WHERE schema_key = 'standalone';
