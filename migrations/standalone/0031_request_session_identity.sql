-- Issue #579 Task 2: mirror the PostgreSQL request session identity columns in
-- the standalone SQLite ledger. `session_header_id` holds the `x-session-id`
-- family value; `session_parent_id` is the child-session link only and never
-- takes part in conversation derivation. Both stay NULL for clients that send
-- no session header (Codex CLI).
ALTER TABLE standalone_usage_summaries ADD COLUMN session_header_id TEXT;
ALTER TABLE standalone_usage_summaries ADD COLUMN session_parent_id TEXT;

UPDATE standalone_schema_meta
SET schema_version = 31
WHERE schema_key = 'standalone';
