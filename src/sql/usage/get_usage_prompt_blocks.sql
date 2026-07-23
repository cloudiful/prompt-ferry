SELECT block_hash, role, content_json, preview_text, created_at
FROM usage_prompt_blocks
WHERE block_hash = ANY($1)
