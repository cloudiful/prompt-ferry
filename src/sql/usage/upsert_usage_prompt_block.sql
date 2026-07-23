INSERT INTO usage_prompt_blocks(block_hash, role, content_json, preview_text)
VALUES ($1, $2, $3, $4)
ON CONFLICT (block_hash) DO NOTHING
