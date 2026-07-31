WITH RECURSIVE replay_chain AS (
    SELECT event_id, parent_event_id
    FROM request_records
    WHERE event_id = $5::BIGINT

    UNION ALL

    SELECT parent.event_id, parent.parent_event_id
    FROM request_records AS parent
    JOIN replay_chain AS child
      ON child.parent_event_id = parent.event_id
)
SELECT
    tool_call.tool_call_event_id,
    tool_call.parent_event_id,
    tool_call.conversation_id,
    tool_call.call_id,
    tool_call.tool_name,
    tool_call.arguments_json,
    tool_call.arguments_preview,
    tool_call.status,
    tool_call.sequence_in_turn,
    tool_call.mcp_request_event_id,
    tool_call.created_at,
    tool_call.updated_at,
    (artifact.event_id IS NOT NULL) AS "has_assistant_artifact!"
FROM request_record_tool_calls AS tool_call
JOIN request_records AS parent
    ON parent.event_id = tool_call.parent_event_id
LEFT JOIN request_record_assistant_artifacts AS artifact
    ON artifact.event_id = tool_call.parent_event_id
   AND parent.content_expired_at IS NULL
WHERE tool_call.call_id = ANY($1)
  AND parent.event_kind = 'request'
  AND parent.request_category = 'ai'
  AND parent.user_id IS NOT DISTINCT FROM $2::BIGINT
  AND parent.endpoint_id IS NOT DISTINCT FROM $3::UUID
  AND (
      $4::UUID IS NULL
      OR parent.conversation_id IS NOT DISTINCT FROM $4::UUID
  )
  AND (
      $5::BIGINT IS NULL
      OR parent.event_id IN (SELECT event_id FROM replay_chain)
  )
ORDER BY tool_call.call_id ASC, tool_call.parent_event_id ASC,
         tool_call.sequence_in_turn ASC NULLS LAST,
         tool_call.tool_call_event_id ASC
