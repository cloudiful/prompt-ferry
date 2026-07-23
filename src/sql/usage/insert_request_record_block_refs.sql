INSERT INTO request_record_block_refs (event_id, block_hash)
SELECT $1, refs.ref->>'block_hash'
FROM jsonb_array_elements($2) refs(ref)
WHERE refs.ref ? 'block_hash'
ON CONFLICT (event_id, block_hash) DO NOTHING;
