-- Issue #69 Phase P7b: bounded raw-body prune for one boundary chunk.
--
-- `$1` is the exact `now - raw_retention_days` cutoff, `$3`/`$4` are the
-- listed chunk range. There is deliberately no lower bound beyond the chunk
-- range and no DEFAULT-partition branch: the whole raw parent is a
-- TimescaleDB hypertable and the row prune must cover the boundary chunk the
-- whole-chunk drop deliberately retained. `FOR UPDATE SKIP LOCKED` keeps
-- concurrent body writes from blocking the prune.
WITH expired AS (
    SELECT created_at, event_id
    FROM request_record_raw_payloads raw
    WHERE raw.created_at < $1
      AND raw.created_at >= $3
      AND raw.created_at < $4
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
      AND rr.created_at >= $3
      AND rr.created_at < $4
    RETURNING rr.event_id
)
SELECT
    COALESCE((SELECT COUNT(*) FROM deleted), 0)::BIGINT AS deleted_count,
    COALESCE((SELECT COUNT(*) FROM cleared), 0)::BIGINT AS cleared_count;
