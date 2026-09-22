WITH expired_conversations AS (
    SELECT rr.conversation_id
    FROM request_records rr
    WHERE rr.conversation_id IS NOT NULL
    GROUP BY rr.conversation_id
    HAVING MAX(rr.created_at) < NOW() - ($1::BIGINT * INTERVAL '1 day')
       AND NOT EXISTS (
           SELECT 1
           FROM request_records active
           WHERE active.conversation_id = rr.conversation_id
             AND (
                 active.request_state IN ('received', 'awaiting_approval', 'upstream_processing')
                 OR active.lease_expires_at > NOW()
             )
       )
), eligible AS (
    SELECT rr.event_id, rr.conversation_id
    FROM request_records rr
    WHERE rr.content_expired_at IS NULL
      AND (
          (
              rr.conversation_id IS NOT NULL
              AND EXISTS (
                  SELECT 1
                  FROM expired_conversations expired
                  WHERE expired.conversation_id = rr.conversation_id
              )
          )
          OR (
              rr.conversation_id IS NULL
              AND rr.created_at < NOW() - ($1::BIGINT * INTERVAL '1 day')
          )
      )
      AND rr.request_state NOT IN ('received', 'awaiting_approval', 'upstream_processing')
      -- NULL means "no lease was ever attached", which must count as inactive:
      -- the bare `NOT (lease_expires_at > NOW())` form evaluates to NULL (not
      -- TRUE) for those rows and silently kept them out of retention forever.
      -- `prune_usage_events.sql` uses the same NULL-safe shape.
      AND (rr.lease_expires_at IS NULL OR rr.lease_expires_at <= NOW())
    ORDER BY rr.created_at ASC, rr.event_id ASC
    LIMIT $2
    FOR UPDATE SKIP LOCKED
), marked AS (
    UPDATE request_records rr
    SET
        content_expired_at = NOW(),
        request_full_json = NULL,
        request_delta_json = NULL,
        response_prompt = NULL,
        upstream_error_body = NULL
    FROM eligible
    WHERE rr.event_id = eligible.event_id
    RETURNING rr.event_id, rr.conversation_id
), deleted_block_refs AS (
    DELETE FROM request_record_block_refs refs
    USING marked
    WHERE refs.event_id = marked.event_id
    RETURNING refs.event_id
), deleted_artifacts AS (
    DELETE FROM request_record_assistant_artifacts artifacts
    USING marked
    WHERE artifacts.event_id = marked.event_id
    RETURNING artifacts.event_id
), deleted_tool_calls AS (
    DELETE FROM request_record_tool_calls calls
    USING marked
    WHERE calls.parent_event_id = marked.event_id
    RETURNING
        calls.tool_call_event_id,
        (calls.arguments_json IS NOT NULL OR calls.arguments_preview IS NOT NULL)
            AS had_arguments
), deleted_snapshots AS (
    DELETE FROM request_record_replay_snapshots snapshots
    USING marked
    WHERE snapshots.event_id = marked.event_id
       OR snapshots.base_event_id = marked.event_id
    RETURNING snapshots.event_id
), deleted_redaction_sessions AS (
    DELETE FROM conversation_redaction_sessions sessions
    USING expired_conversations expired
    WHERE sessions.conversation_id = expired.conversation_id
    RETURNING sessions.conversation_id
), deleted_idle_sessions AS (
    -- Issue #524 Task 6: bound session storage independently of content
    -- retention. Sessions idle for over a week are dead weight even when their
    -- conversation has no expired request rows yet. The NOT EXISTS keeps each
    -- row owned by exactly one data-modifying CTE in this statement.
    DELETE FROM conversation_redaction_sessions sessions
    WHERE sessions.updated_at < NOW() - INTERVAL '7 days'
      AND NOT EXISTS (
          SELECT 1
          FROM expired_conversations expired
          WHERE expired.conversation_id = sessions.conversation_id
      )
    RETURNING sessions.conversation_id
)
SELECT
    (SELECT COUNT(*) FROM marked)::BIGINT AS "expired_events!",
    (SELECT COUNT(*) FROM deleted_block_refs)::BIGINT AS "deleted_block_refs!",
    (SELECT COUNT(*) FROM deleted_artifacts)::BIGINT AS "deleted_artifacts!",
    (SELECT COUNT(*) FROM deleted_tool_calls)::BIGINT AS "deleted_tool_calls!",
    (SELECT COUNT(*) FROM deleted_snapshots)::BIGINT AS "deleted_snapshots!",
    (SELECT COUNT(*) FROM deleted_tool_calls WHERE had_arguments)::BIGINT AS "cleared_tool_arguments!",
    (
        (SELECT COUNT(*) FROM deleted_redaction_sessions)
        + (SELECT COUNT(*) FROM deleted_idle_sessions)
    )::BIGINT AS "deleted_redaction_sessions!"
