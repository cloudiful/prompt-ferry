-- Scoped orphan sweep for a bounded clear.
--
-- Only blocks captured from this clear's candidates are considered, and only
-- blocks created before the current UTC day. A block survives when any ref
-- remains, including refs written by another user after the family delete
-- (READ COMMITTED sees them). Unrelated preexisting orphans are left for the
-- normal retention/full-history paths.
--
--   $1  captured candidate block hashes
DELETE FROM usage_prompt_blocks upb
WHERE upb.block_hash = ANY($1::TEXT[])
  AND upb.created_at < (date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC')
  AND NOT EXISTS (
      SELECT 1
      FROM request_record_block_refs ref
      WHERE ref.block_hash = upb.block_hash
  );
