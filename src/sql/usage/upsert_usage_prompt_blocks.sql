INSERT INTO usage_prompt_blocks(created_at, block_hash, role, content_json, preview_text)
SELECT $1::timestamptz AS created_at, block_hash, role, content_json->'value', preview_text
FROM jsonb_to_recordset($2::jsonb) AS blocks(block_hash text, role text, content_json jsonb, preview_text text)
ORDER BY block_hash, created_at
ON CONFLICT (block_hash, created_at) DO NOTHING
