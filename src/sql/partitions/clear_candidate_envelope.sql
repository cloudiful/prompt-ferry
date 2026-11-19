-- Inclusive created_at envelope of the clear candidates.
--
-- The predicate is exactly the candidate predicate of clear_family_rows.sql
-- (range plus scope, without the redundant envelope quals). An empty result
-- means the clear has nothing to touch: no chunk decompression, no family
-- delete and no orphan sweep are run.
--
--   $1  target_user_id (TargetUser scope; NULL otherwise)
--   $2  visible_user_id (CurrentUser scope; NULL otherwise)
--   $3  start_at (NULL = unbounded)
--   $4  end_at (NULL = unbounded, inclusive)
--   $5  scope code: 0 = all users, 1 = current user, 2 = target user
SELECT
    MIN(rr.created_at) AS min_created_at,
    MAX(rr.created_at) AS max_created_at
FROM request_records rr
WHERE ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
  AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at <= $4)
  AND (
      $5::INT = 0
      OR ($5::INT = 1 AND rr.user_id = $2::BIGINT)
      OR ($5::INT = 2 AND rr.user_id = $1::BIGINT)
  );
