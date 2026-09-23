SELECT COUNT(*)::BIGINT AS "count!"
FROM usage_prompt_blocks
WHERE block_hash = $1;
