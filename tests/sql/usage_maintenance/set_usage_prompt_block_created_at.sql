UPDATE usage_prompt_blocks
SET created_at = $2
WHERE block_hash = $1;
