-- Prompt blocks referenced by the clear candidates, captured before the
-- family delete removes those refs.
--
-- The block set is scoped to the candidates through the same ref join the
-- family delete uses; it is not the global orphan population. Prompt blocks
-- created today stay in the hot, uncompressed chunk and are not selected.
--
--   $1  target_user_id (TargetUser scope; NULL otherwise)
--   $2  visible_user_id (CurrentUser scope; NULL otherwise)
--   $3  start_at (NULL = unbounded)
--   $4  end_at (NULL = unbounded, inclusive)
--   $5  scope code: 0 = all users, 1 = current user, 2 = target user
SELECT DISTINCT upb.block_hash AS "block_hash!"
FROM request_records rr
JOIN request_record_block_refs refs
  ON refs.event_id = rr.event_id
 AND refs.created_at = rr.created_at
JOIN usage_prompt_blocks upb
  ON upb.block_hash = refs.block_hash
WHERE ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3)
  AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at <= $4)
  AND (
      $5::INT = 0
      OR ($5::INT = 1 AND rr.user_id = $2::BIGINT)
      OR ($5::INT = 2 AND rr.user_id = $1::BIGINT)
  )
  AND upb.created_at < (date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC')
ORDER BY upb.block_hash;
