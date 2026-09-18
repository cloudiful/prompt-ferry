-- Issue #502 Task 5: mirror PostgreSQL 0092 per-target compact mode
-- for the standalone SQLite store. `compact_mode` is TEXT NOT NULL
-- DEFAULT 'passthrough'; `self_summarize` enables ferry-side handoff
-- summarization for non-Responses targets; `off` rejects compact
-- explicitly. Missing values on pre-migration rows read as `passthrough`
-- (see `rows::route_target`).
ALTER TABLE standalone_model_route_targets
ADD COLUMN compact_mode TEXT NOT NULL DEFAULT 'passthrough';

UPDATE standalone_schema_meta
SET schema_version = 28
WHERE schema_key = 'standalone';
