INSERT INTO request_record_block_refs (event_id, block_hash)
SELECT $1, upb.block_hash
FROM jsonb_array_elements($2) refs(ref)
JOIN usage_prompt_blocks upb
    ON upb.block_hash = refs.ref->>'block_hash'
WHERE refs.ref ? 'block_hash'
ON CONFLICT (event_id, block_hash) DO NOTHING;
