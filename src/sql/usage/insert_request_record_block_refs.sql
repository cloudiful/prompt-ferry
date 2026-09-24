-- Issue #277 Phase P7: the block-existence JOIN was a FK-race workaround; with
-- the loose reference gone, refs are inserted directly and the prompt block is
-- content-addressed per day.
INSERT INTO request_record_block_refs (created_at, event_id, block_hash)
SELECT $1, $2, refs.ref->>'block_hash'
FROM jsonb_array_elements($3) refs(ref)
WHERE refs.ref ? 'block_hash'
ON CONFLICT (event_id, block_hash, created_at) DO NOTHING;
