-- Issue #277 Phase P8: prompt blocks are day-partitioned, so the orphan
-- guard is the current UTC day — a block written today is never reaped
-- mid-day, and blocks from earlier days vanish with their partitions unless
-- this guard reaps them as orphans first.
DELETE FROM usage_prompt_blocks upb
WHERE upb.created_at < (date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC')
  AND NOT EXISTS (
      SELECT 1
      FROM request_record_block_refs ref
      WHERE ref.block_hash = upb.block_hash
  );
