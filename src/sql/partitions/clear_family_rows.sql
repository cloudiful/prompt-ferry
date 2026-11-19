-- Scoped row deletion across the whole request family.
--
-- Only the manual admin clear endpoint reaches this statement: the 15-minute
-- tick never deletes request-family rows (it drops whole chunks instead).
-- `clear_family_rows` calls it either with an explicit scope/range (per-user
-- or windowed clear) or, for a full-history clear, with scope=all and the
-- current UTC day as `start_at` so only today's rows remain to delete after
-- every earlier chunk was dropped.
--
-- $6/$7 are the inclusive envelope of the same candidates. They are logically
-- redundant with the row predicates, but stating them on the parent and on
-- every arm lets TimescaleDB exclude chunks during startup instead of reading
-- chunks the envelope cannot touch.
--
-- Scope semantics:
--   $1  target_user_id (TargetUser scope; NULL otherwise)
--   $2  visible_user_id (CurrentUser scope; NULL otherwise)
--   $3  start_at (NULL = unbounded)
--   $4  end_at (NULL = unbounded, inclusive)
--   $5  scope code: 0 = all users, 1 = current user, 2 = target user
--   $6  envelope min created_at (inclusive)
--   $7  envelope max created_at (inclusive)
WITH candidates AS MATERIALIZED (
    SELECT rr.event_id, rr.created_at
    FROM request_records rr
    WHERE ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at <= $4)
      AND rr.created_at >= $6::TIMESTAMPTZ
      AND rr.created_at <= $7::TIMESTAMPTZ
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
      AND rr.created_at >= $6::TIMESTAMPTZ
      AND rr.created_at <= $7::TIMESTAMPTZ
    RETURNING rr.event_id
), deleted_content AS (
    DELETE FROM request_record_content content
    USING candidates
    WHERE content.event_id = candidates.event_id
      AND content.created_at = candidates.created_at
      AND content.created_at >= $6::TIMESTAMPTZ
      AND content.created_at <= $7::TIMESTAMPTZ
    RETURNING content.event_id
), deleted_block_refs AS (
    DELETE FROM request_record_block_refs refs
    USING candidates
    WHERE refs.event_id = candidates.event_id
      AND refs.created_at = candidates.created_at
      AND refs.created_at >= $6::TIMESTAMPTZ
      AND refs.created_at <= $7::TIMESTAMPTZ
    RETURNING refs.event_id
), deleted_artifacts AS (
    DELETE FROM request_record_assistant_artifacts artifacts
    USING candidates
    WHERE artifacts.event_id = candidates.event_id
      AND artifacts.created_at = candidates.created_at
      AND artifacts.created_at >= $6::TIMESTAMPTZ
      AND artifacts.created_at <= $7::TIMESTAMPTZ
    RETURNING artifacts.event_id
), deleted_tool_calls AS (
    DELETE FROM request_record_tool_calls calls
    USING candidates
    WHERE calls.parent_event_id = candidates.event_id
      AND calls.created_at = candidates.created_at
      AND calls.created_at >= $6::TIMESTAMPTZ
      AND calls.created_at <= $7::TIMESTAMPTZ
    RETURNING calls.tool_call_event_id
), deleted_snapshots AS (
    DELETE FROM request_record_replay_snapshots snapshots
    USING candidates
    WHERE snapshots.created_at = candidates.created_at
      AND snapshots.created_at >= $6::TIMESTAMPTZ
      AND snapshots.created_at <= $7::TIMESTAMPTZ
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
      AND raw.created_at >= $6::TIMESTAMPTZ
      AND raw.created_at <= $7::TIMESTAMPTZ
    RETURNING raw.event_id
)
SELECT
    (SELECT COUNT(*) FROM deleted_metadata)::BIGINT AS "deleted_count!",
    (SELECT MIN(created_at) FROM candidates) AS min_created_at,
    (SELECT MAX(created_at) FROM candidates) AS max_created_at;
