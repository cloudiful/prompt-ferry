WITH expired AS (
    SELECT created_at, event_id
    FROM request_record_raw_payloads raw
    WHERE raw.created_at < $1
      AND (
          raw.created_at >= $3
          OR raw.tableoid = 'request_record_raw_payloads_default'::regclass
      )
    ORDER BY created_at, event_id
    LIMIT $2
    FOR UPDATE SKIP LOCKED
), deleted AS (
    DELETE FROM request_record_raw_payloads raw
    USING expired
    WHERE raw.created_at = expired.created_at
      AND raw.event_id = expired.event_id
    RETURNING raw.event_id
), cleared AS (
    UPDATE request_records rr
    SET
    request_has_previous_response_id = FALSE,
    request_previous_response_id = NULL,
    request_previous_response_parent_found = NULL,
    request_conversation_key = NULL,
    request_conversation_parent_found = NULL
    WHERE rr.event_id = ANY (ARRAY(SELECT event_id FROM deleted))
    RETURNING rr.event_id
)
SELECT
    COALESCE((SELECT COUNT(*) FROM deleted), 0)::BIGINT AS deleted_count,
    COALESCE((SELECT COUNT(*) FROM cleared), 0)::BIGINT AS cleared_count;
