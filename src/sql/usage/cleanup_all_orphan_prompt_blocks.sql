-- Issue #277 Phase P8: full-history clear reaps every orphan prompt block.
-- After the history partitions are dropped and today's block refs are
-- deleted, a prompt block with no remaining ref is dead for every scope.
DELETE FROM usage_prompt_blocks upb
WHERE NOT EXISTS (
    SELECT 1
    FROM request_record_block_refs ref
    WHERE ref.block_hash = upb.block_hash
);
