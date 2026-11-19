-- Issue #69 Phase P4b: delete only redaction sessions orphaned by a clear.
--
-- Unlike the tick's stale-session cleanup, this deliberately ignores the
-- seven-day idle predicate: a live session that merely looks old belongs to a
-- user who is still working, while a session whose last request row is gone
-- can never be resumed.
DELETE FROM conversation_redaction_sessions sessions
WHERE sessions.last_event_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
      FROM request_records rr
      WHERE rr.event_id = sessions.last_event_id
  );
