-- Issue #277 Phase P7: prompt blocks carry a day-granular `created_at`, so the
-- orphan guard is the current UTC day instead of a minute-level grace window:
-- a block written today is never reaped mid-day, older orphans are.
DELETE FROM usage_prompt_blocks upb
WHERE upb.created_at < (date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC')
  AND NOT EXISTS (
      SELECT 1
      FROM request_record_block_refs ref
      WHERE ref.block_hash = upb.block_hash
  );
