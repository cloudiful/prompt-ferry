-- Issue #277 Phase P8: scoped row deletion across the whole request family.
--
-- Only the manual admin clear endpoint reaches this statement: the 15-minute
-- tick never deletes request-family rows (it drops whole partitions instead).
-- `clear_usage_events` calls it either with an explicit scope/range (per-user
-- or windowed clear) or, for a full-history clear, with scope=all and the
-- current UTC day as `start_at` so only today's rows remain to delete after
-- every earlier partition was dropped.
--
-- Billing protection is gone by decision: the usage_charges ledger is
-- permanent and decoupled from the request family.
--
-- Scope semantics mirror the legacy clear SQL:
--   $1  target_user_id (TargetUser scope; NULL otherwise)
--   $2  visible_user_id (CurrentUser scope; NULL otherwise)
--   $3  start_at (NULL = unbounded)
--   $4  end_at (NULL = unbounded)
--   $5  scope code: 0 = all users, 1 = current user, 2 = target user
WITH candidates AS MATERIALIZED (
    SELECT rr.event_id, rr.created_at
    FROM request_records rr
    WHERE ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at <= $4)
      AND (
          $5::INT = 0
          OR ($5::INT = 1 AND rr.user_id = $2::BIGINT)
          OR ($5::INT = 2 AND rr.user_id = $1::BIGINT)
      )
), deleted_metadata AS (
    DELETE FROM request_records rr
    USING candidates
    WHERE rr.event_id = candidates.event_id
      AND rr.created_at = candidates.created_at
    RETURNING rr.event_id
), deleted_content AS (
    DELETE FROM request_record_content content
    USING candidates
    WHERE content.event_id = candidates.event_id
      AND content.created_at = candidates.created_at
    RETURNING content.event_id
), deleted_block_refs AS (
    DELETE FROM request_record_block_refs refs
    USING candidates
    WHERE refs.event_id = candidates.event_id
      AND refs.created_at = candidates.created_at
    RETURNING refs.event_id
), deleted_artifacts AS (
    DELETE FROM request_record_assistant_artifacts artifacts
    USING candidates
    WHERE artifacts.event_id = candidates.event_id
      AND artifacts.created_at = candidates.created_at
    RETURNING artifacts.event_id
), deleted_tool_calls AS (
    DELETE FROM request_record_tool_calls calls
    USING candidates
    WHERE calls.parent_event_id = candidates.event_id
      AND calls.created_at = candidates.created_at
    RETURNING calls.tool_call_event_id
), deleted_snapshots AS (
    DELETE FROM request_record_replay_snapshots snapshots
    USING candidates
    WHERE snapshots.created_at = candidates.created_at
      AND (
          snapshots.event_id = candidates.event_id
          OR snapshots.base_event_id = candidates.event_id
      )
    RETURNING snapshots.event_id
), deleted_raw_payloads AS (
    DELETE FROM request_record_raw_payloads raw
    USING candidates
    WHERE raw.event_id = candidates.event_id
      AND raw.created_at = candidates.created_at
    RETURNING raw.event_id
)
SELECT (SELECT COUNT(*) FROM deleted_metadata)::BIGINT AS "deleted_count!";
