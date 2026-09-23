-- Issue #579 Task 2: the session identity a request carried, persisted on every
-- request record so the admin console can attribute records to their session
-- and link a child session to its parent. `session_parent_id` is the
-- child-session link only and never takes part in conversation derivation.
-- Both columns stay NULL for clients that send no session header (Codex CLI).
ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS session_header_id TEXT NULL;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS session_parent_id TEXT NULL;
