-- Issue #277 Phase P8: conversation_redaction_sessions is a plain table, so
-- the partition tick must bound it explicitly. A session is dead when it has
-- been idle for over a week (issue #524 Task 6) or when the request event it
-- last touched left with a dropped metadata partition.
DELETE FROM conversation_redaction_sessions sessions
WHERE sessions.updated_at < NOW() - INTERVAL '7 days'
   OR (
       sessions.last_event_id IS NOT NULL
       AND NOT EXISTS (
           SELECT 1
           FROM request_records rr
           WHERE rr.event_id = sessions.last_event_id
       )
   );
