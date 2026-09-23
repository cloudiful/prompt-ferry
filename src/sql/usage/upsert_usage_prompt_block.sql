-- Issue #277 Phase P7: prompt blocks are partitioned by day, so dedupe is per
-- `(block_hash, created_at day)`; the caller passes UTC midnight as
-- `created_at` so a block is stored once per day instead of once per request.
INSERT INTO usage_prompt_blocks(created_at, block_hash, role, content_json, preview_text)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (block_hash, created_at) DO NOTHING
